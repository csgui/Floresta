// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Guile thread: startup, failure isolation and shutdown.
//!
//! # Isolation
//!
//! Dossel is an extension layer, not part of the node. A bug in it — a broken
//! init file, a panic in a conversion routine, a REPL user calling something
//! that misbehaves — must never take the node down. Three layers enforce that:
//!
//! 1. Each Scheme primitive runs inside a `catch_unwind` (see
//!    [`super::module::guard`]), so a Rust panic becomes a Scheme error and the
//!    session survives.
//! 2. Everything the Guile thread does runs inside a second `catch_unwind`
//!    here, so a panic that escapes a primitive ends the *thread*, not the
//!    process.
//! 3. The thread is then restarted, with backoff, up to [`MAX_RESTARTS`].
//!
//! The node itself never waits on this thread and never observes its state.

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::module;
use super::repl;
use super::safe;
use crate::error::DosselError;

/// How often the Guile thread wakes to check for shutdown.
///
/// The thread has nothing else to do — `spawn-server` serves clients on its own
/// threads — so this only bounds how long `shutdown` takes to return.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How many times a failed Guile thread is restarted before giving up.
///
/// Bounded because a failure that recurs is almost certainly deterministic —
/// a malformed init file, a missing socket directory — and retrying it forever
/// would fill the log without ever succeeding. Giving up leaves the node
/// running happily without a REPL, which is the right trade.
const MAX_RESTARTS: u32 = 5;

/// Backoff before restarting a failed Guile thread.
const RESTART_BACKOFF: Duration = Duration::from_secs(2);

/// How Dossel should start up.
#[derive(Debug, Clone)]
pub struct DosselConfig {
    /// Where to listen for REPL clients.
    pub socket_path: PathBuf,

    /// A Scheme file to load at startup, if any.
    ///
    /// Definitions made here live in the node's Guile environment and are
    /// therefore visible to every REPL session, including ones opened long
    /// afterwards. This is `florestad --load`.
    pub init_file: Option<PathBuf>,
}

impl DosselConfig {
    /// The default layout for a node with the given data directory: a
    /// `repl.sock` beside the rest of the node's state.
    pub fn from_datadir(datadir: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: datadir.into().join("repl.sock"),
            init_file: None,
        }
    }
}

/// A handle to the running Dossel extension layer.
///
/// Dropping this does **not** stop the REPL; call [`DosselRuntime::shutdown`]
/// for that. The handle is detached on purpose, so that a caller who does not
/// care about orderly shutdown cannot accidentally kill live sessions by
/// letting a binding go out of scope.
pub struct DosselRuntime {
    shutdown: Arc<AtomicBool>,
    socket_path: PathBuf,
    thread: Option<thread::JoinHandle<()>>,
}

impl DosselRuntime {
    /// Start the Guile thread and the REPL server.
    ///
    /// Returns as soon as the thread is spawned. Startup failures — a socket
    /// that cannot be bound, an init file with a syntax error — are logged by
    /// the thread rather than returned here, because they must not be able to
    /// abort node startup.
    pub(crate) fn start(config: DosselConfig) -> Result<Self, DosselError> {
        // The one thing worth failing early on: if the path is unusable, the
        // operator should hear about it now, from the call site that knows
        // where the path came from.
        repl::prepare_socket_path(&config.socket_path)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let socket_path = config.socket_path.clone();

        let thread = thread::Builder::new()
            .name("dossel-guile".to_owned())
            .spawn({
                let shutdown = Arc::clone(&shutdown);
                move || supervise(config, &shutdown)
            })
            .map_err(DosselError::ThreadSpawn)?;

        Ok(Self {
            shutdown,
            socket_path,
            thread: Some(thread),
        })
    }

    /// Where the REPL is listening.
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }

    /// Stop the REPL, disconnect clients and join the Guile thread.
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);

        if let Some(thread) = self.thread.take() {
            // A panicking Guile thread is already handled inside `supervise`,
            // so a join error here means something truly unexpected. Log it and
            // carry on with node shutdown regardless.
            if thread.join().is_err() {
                warn!("Dossel Guile thread panicked during shutdown");
            }
        }

        // Leaving a stale socket behind would make the next start log a
        // confusing "address in use" if the removal in `prepare_socket_path`
        // ever failed.
        let _ = std::fs::remove_file(&self.socket_path);

        info!("Dossel REPL stopped");
    }
}

/// Run the Guile thread, restarting it if it fails.
fn supervise(config: DosselConfig, shutdown: &AtomicBool) {
    let mut restarts = 0_u32;

    while !shutdown.load(Ordering::SeqCst) {
        match run_in_guile(&config, shutdown) {
            Ok(()) => {
                // A clean return means shutdown was requested.
                debug!("Dossel Guile thread finished cleanly");
                return;
            }
            Err(reason) => {
                if shutdown.load(Ordering::SeqCst) {
                    return;
                }

                restarts += 1;
                if restarts > MAX_RESTARTS {
                    error!(
                        "Dossel Guile thread failed {MAX_RESTARTS} times ({reason}); giving up. \
                         The node continues to run without a REPL."
                    );
                    return;
                }

                warn!(
                    "Dossel Guile thread failed ({reason}); restarting \
                     ({restarts}/{MAX_RESTARTS}) in {:?}",
                    RESTART_BACKOFF
                );
                thread::sleep(RESTART_BACKOFF);
            }
        }
    }
}

/// Enter Guile mode and run [`guile_main`], converting a panic into an error.
///
/// The `catch_unwind` sits *inside* [`safe::with_guile`] because the frame
/// `scm_with_guile` calls is `extern "C"`; letting an unwind reach it would
/// abort the process rather than merely kill the thread.
fn run_in_guile(config: &DosselConfig, shutdown: &AtomicBool) -> Result<(), String> {
    let mut outcome = Err("the Guile thread exited without reporting a result".to_owned());

    safe::with_guile(|| {
        // `AssertUnwindSafe` is sound here: the only state shared with the rest
        // of the process is the shutdown flag, an `AtomicBool` that cannot be
        // left in a broken state by a panic.
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| guile_main(config, shutdown)));

        outcome = match result {
            Ok(inner) => inner,
            Err(payload) => Err(format!("panic in the Guile thread: {}", panic_text(&payload))),
        };
    });

    outcome
}

/// Everything the Guile thread does, from inside Guile mode.
fn guile_main(config: &DosselConfig, shutdown: &AtomicBool) -> Result<(), String> {
    // On a restart there may be a server still bound from the previous
    // incarnation. Stopping it is best effort: on a first run there is nothing
    // to stop and this reports an error we do not care about.
    let _ = repl::stop();

    module::define().map_err(|e| format!("could not define the (floresta node) module: {e}"))?;

    module::import_into_user_module()
        .map_err(|e| format!("could not import (floresta node) into (guile-user): {e}"))?;

    if let Some(init_file) = &config.init_file {
        load_init_file(init_file)?;
    }

    // Re-prepare the path: on a restart the previous socket file is still on
    // disk, and `bind(2)` would fail with EADDRINUSE.
    repl::prepare_socket_path(&config.socket_path).map_err(|e| e.to_string())?;

    repl::start(&config.socket_path).map_err(|e| e.to_string())?;

    info!(
        "Dossel REPL listening on {}",
        config.socket_path.display()
    );

    // Nothing left to do but wait: clients are served on Guile's own threads.
    while !shutdown.load(Ordering::SeqCst) {
        thread::sleep(SHUTDOWN_POLL_INTERVAL);
    }

    if let Err(e) = repl::stop() {
        debug!("Dossel REPL server did not stop cleanly: {e}");
    }

    Ok(())
}

/// Load the `--load` startup file.
///
/// A missing file is an error worth reporting: the operator asked for it
/// explicitly on the command line, so silently ignoring it would hide a typo.
/// A *broken* file is reported but not fatal — the REPL still comes up, which
/// is exactly where the operator needs to be to fix it.
fn load_init_file(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("startup file {} does not exist", path.display()));
    }

    let path_str = path
        .to_str()
        .ok_or_else(|| format!("startup file path {} is not valid UTF-8", path.display()))?;

    match safe::load_file(path_str) {
        Ok(_) => {
            info!("Dossel loaded startup file {}", path.display());
            Ok(())
        }
        Err(e) => {
            error!(
                "Dossel startup file {} raised an error: {e}. The REPL will start anyway; \
                 definitions from the file may be missing.",
                path.display()
            );
            Ok(())
        }
    }
}

/// Best-effort rendering of a panic payload.
fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_owned())
}
