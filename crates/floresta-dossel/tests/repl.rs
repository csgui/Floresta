// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end tests: start a real Guile interpreter, serve a real REPL over a
//! real Unix socket, and talk to it the way `floresta-repl` does.
//!
//! # Why one shared node
//!
//! Dossel's attachment to a node is process-global (see `guile::bridge` for
//! why), and `cargo test` runs every test in one process. So these tests share
//! a single [`DosselRuntime`], started once on first use. Each test opens its
//! own REPL session against it, which is exactly the concurrency the real thing
//! has to support anyway.
//!
//! # Current surface
//!
//! The primitive surface is deliberately minimal and grown one procedure at a
//! time from a from-scratch redesign (see `guile/module.rs`): `get-block-height`,
//! `rpc-call`, `get-config` and `set-config!` so far. These tests cover exactly
//! that, plus the REPL mechanics themselves — session independence, error
//! resilience, socket permissions — not a wide API surface, because there
//! isn't one yet.

use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use floresta_dossel::ConfigKey;
use floresta_dossel::ConfigValue;
use floresta_dossel::DosselConfig;
use floresta_dossel::DosselRuntime;
use floresta_dossel::RuntimeConfig;
use floresta_dossel::config::FnBackend;
use floresta_dossel::config::StaticBackend;
use floresta_dossel::testing::MockApi;

/// How long to wait for the REPL to produce a prompt before failing a test.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Serializes the tests that mutate shared node state.
///
/// `ban-threshold` is backed by one cell shared by every session — that is the
/// point of it — so two tests writing it concurrently would race. Reading tests
/// need no lock.
static MUTATES_CONFIG: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Harness {
    socket: PathBuf,
    // Held so the runtime is never dropped for the life of the test process.
    _runtime: DosselRuntime,
    _tokio: tokio::runtime::Runtime,
}

fn harness() -> &'static Harness {
    static HARNESS: OnceLock<Harness> = OnceLock::new();

    HARNESS.get_or_init(|| {
        // Kept short: a Unix socket path has a hard length limit, and macOS
        // temp directories are long enough to bump into it.
        let dir = PathBuf::from(format!("/tmp/dossel-test-{}", std::process::id()));
        let socket = dir.join("repl.sock");

        let config = RuntimeConfig::new();
        config.bind(
            ConfigKey::Network,
            Arc::new(StaticBackend::new(ConfigValue::Symbol("regtest".to_owned()))),
        );
        config.bind(
            ConfigKey::Datadir,
            Arc::new(StaticBackend::new(ConfigValue::Str(
                dir.display().to_string(),
            ))),
        );
        config.bind(
            ConfigKey::Version,
            Arc::new(StaticBackend::new(ConfigValue::Str("0.9.0-test".to_owned()))),
        );

        // A genuinely writable key, so the tests can exercise the write path:
        // validation, the backend call, and reading the new value back. In
        // florestad this key is bound read-only, because Floresta has no way to
        // change `max_banscore` on a running node.
        let ban_threshold = Arc::new(std::sync::atomic::AtomicI64::new(100));
        config.bind(
            ConfigKey::BanThreshold,
            Arc::new(FnBackend::read_write(
                {
                    let cell = Arc::clone(&ban_threshold);
                    move || {
                        Ok(ConfigValue::Integer(i128::from(
                            cell.load(std::sync::atomic::Ordering::SeqCst),
                        )))
                    }
                },
                {
                    let cell = Arc::clone(&ban_threshold);
                    move |value| {
                        let ConfigValue::Integer(n) = value else {
                            unreachable!("ban-threshold is validated as an integer")
                        };
                        cell.store(n as i64, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    }
                },
            )),
        );

        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("could not build the test Tokio runtime");

        let runtime = tokio.block_on(async {
            DosselRuntime::spawn(
                DosselConfig {
                    socket_path: socket.clone(),
                    init_file: None,
                },
                Arc::new(MockApi::default()),
                config,
            )
            .expect("Dossel failed to start")
        });

        wait_for_socket(&socket);

        Harness {
            socket,
            _runtime: runtime,
            _tokio: tokio,
        }
    })
}

fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + READ_TIMEOUT;
    while Instant::now() < deadline {
        if path.exists() && UnixStream::connect(path).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("REPL socket {} never appeared", path.display());
}

/// Connect, retrying briefly on refusal.
///
/// Guile's REPL server accepts on a thread of its own, so a burst of clients
/// arriving at once can transiently exhaust the listen backlog and get
/// `ECONNREFUSED`. A real interactive client retries by being typed again; the
/// tests, which open a dozen sessions simultaneously, have to do it explicitly.
fn connect_with_retry(path: &Path) -> UnixStream {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut last = None;

    while Instant::now() < deadline {
        match UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    panic!("could not connect to the REPL at {}: {last:?}", path.display());
}

/// Strip ANSI CSI escape sequences (color codes, clear-screen, cursor moves).
///
/// `repl.scm` colors its banner and prompt, and Guile's nested debug REPL
/// (entered on an uncaught error) re-triggers that banner, since it is a
/// distinct `repl` object from Guile's point of view. Stripping escapes here
/// means prompt detection and content assertions depend on the actual text,
/// not on whatever palette `repl.scm` currently uses.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// One REPL session, mirroring what an operator's client does.
struct Session {
    stream: UnixStream,
}

impl Session {
    fn open() -> Self {
        let stream = connect_with_retry(&harness().socket);
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .expect("could not set a read timeout");

        let mut session = Self { stream };
        // Consume the banner and first prompt.
        session.read_to_prompt();
        session
    }

    /// Evaluate `expr` and return everything printed before the next prompt.
    fn eval(&mut self, expr: &str) -> String {
        writeln!(self.stream, "{expr}").expect("could not write to the REPL");
        self.stream.flush().expect("could not flush the REPL");
        self.read_to_prompt()
    }

    /// Read until the REPL asks for more input, i.e. the text (with ANSI
    /// escapes stripped) ends in `"> "` — `dossel> ` normally, or the same
    /// tail on Guile's nested debug prompt after an error.
    fn read_to_prompt(&mut self) -> String {
        let mut out = String::new();
        let mut byte = [0_u8; 1];
        let deadline = Instant::now() + READ_TIMEOUT;

        while Instant::now() < deadline {
            match self.stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    out.push(byte[0] as char);
                    let stripped = strip_ansi(&out);
                    if stripped.ends_with("> ") {
                        return stripped;
                    }
                }
                Err(e) => panic!("REPL read failed after {out:?}: {e}"),
            }
        }

        panic!("REPL produced no prompt within {READ_TIMEOUT:?}; got {out:?}");
    }
}

// ---------------------------------------------------------------------------

#[test]
fn chain_queries_answer_from_the_node() {
    let mut s = Session::open();

    assert!(
        s.eval("(get-block-height)").contains("840443"),
        "expected the mock tip height"
    );
}

#[test]
fn a_scheme_error_does_not_end_the_session_or_the_node() {
    let mut s = Session::open();

    // Unbound variable: a plain typo.
    s.eval("(this-procedure-does-not-exist)");
    // Guile drops into a nested REPL on error; leave it.
    s.eval(",q");

    assert!(
        s.eval("(get-block-height)").contains("840443"),
        "the session must survive an error"
    );
}

#[test]
fn sessions_are_independent() {
    let mut first = Session::open();
    let mut second = Session::open();

    // Acceptance criterion 5: a second client connects while the first is live.
    first.eval("(define only-in-first 41)");
    assert!(first.eval("(+ only-in-first 1)").contains("42"));

    // Both see the node, and both see module-level definitions.
    assert!(second.eval("(get-block-height)").contains("840443"));
    assert!(first.eval("(get-block-height)").contains("840443"));

    // Quitting one leaves the other working (acceptance criterion 4).
    drop(second);
    assert!(first.eval("(get-block-height)").contains("840443"));
}

#[test]
fn rpc_passthrough_round_trips_json() {
    let mut s = Session::open();

    let out = s.eval(r#"(assq-ref (rpc-call "getblockcount") 'method)"#);
    assert!(out.contains("getblockcount"), "got {out}");

    // A JSON array comes back as a Scheme list.
    let params = s.eval(r#"(assq-ref (rpc-call "getblockhash" '(7)) 'params)"#);
    assert!(params.contains('7'), "got {params}");
}

#[test]
fn read_only_config_can_be_read_but_not_written() {
    let mut s = Session::open();

    assert!(s.eval("(get-config 'network)").contains("regtest"));

    let denied = s.eval("(set-config! 'network 'mainnet)");
    assert!(
        denied.contains("read-only"),
        "network must be rejected as read-only, got {denied}"
    );
}

#[test]
fn unbound_config_keys_explain_themselves() {
    let mut s = Session::open();

    // The spec's headline example. It cannot work in this build, and the error
    // has to say why rather than pretending the write succeeded.
    let out = s.eval("(set-config! 'max-peers 32)");
    assert!(
        out.contains("MAX_OUTGOING_PEERS"),
        "max-peers must name the compile-time const, got {out}"
    );

    let log = s.eval("(set-config! 'log-level 'debug)");
    assert!(
        log.contains("no log-level backend"),
        "log-level must explain the missing backend, got {log}"
    );
}

#[test]
fn a_bound_key_can_be_written_and_read_back() {
    let _guard = MUTATES_CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::open();

    assert!(s.eval("(get-config 'ban-threshold)").contains("100"));
    assert!(s.eval("(set-config! 'ban-threshold 50)").contains("ok"));
    assert!(
        s.eval("(get-config 'ban-threshold)").contains("50"),
        "a write must be visible to the next read, in the same session"
    );

    // ...and to a different session, since the backend is the node's state and
    // not per-session Scheme state.
    let mut other = Session::open();
    assert!(other.eval("(get-config 'ban-threshold)").contains("50"));

    // Restore, so test ordering cannot matter.
    other.eval("(set-config! 'ban-threshold 100)");
}

#[test]
fn out_of_range_values_are_rejected() {
    let _guard = MUTATES_CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::open();

    let negative = s.eval("(set-config! 'ban-threshold -1)");
    assert!(
        negative.contains("non-negative"),
        "expected a range complaint, got {negative}"
    );
    s.eval(",q");

    let wrong_type = s.eval(r#"(set-config! 'ban-threshold "many")"#);
    assert!(
        wrong_type.contains("exact non-negative integer"),
        "expected a type complaint, got {wrong_type}"
    );
    s.eval(",q");

    // The value survived both rejections.
    assert!(s.eval("(get-config 'ban-threshold)").contains("100"));
}

#[test]
fn socket_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let socket = &harness().socket;
    let mode = std::fs::metadata(socket)
        .expect("socket must exist")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(mode, 0o600, "REPL socket must not be reachable by others");

    let dir_mode = std::fs::metadata(socket.parent().expect("socket has a parent"))
        .expect("socket directory must exist")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(dir_mode, 0o700, "REPL socket directory must be owner-only");
}
