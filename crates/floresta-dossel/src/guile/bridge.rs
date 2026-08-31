// SPDX-License-Identifier: MIT OR Apache-2.0

//! The bridge from Guile's synchronous world into Floresta's async one.
//!
//! # Why a global, not a thread-local
//!
//! The obvious design is a `thread_local!` holding the node handle, set once
//! when the Guile thread starts. It does not work here. `spawn-server` creates
//! a **new Guile thread per REPL client**, and the primitives registered by
//! [`super::module`] run on those threads, not on the one Dossel started. A
//! thread-local would be empty for every real REPL session and populated only
//! on the thread nobody evaluates anything on.
//!
//! The attachment is therefore process-global. That is sound because
//! [`FlorestaExtensionApi`] is `Send + Sync`, and it is the correct scope
//! anyway: there is one node per process.
//!
//! # Why `Handle::block_on`, not `block_in_place`
//!
//! [`tokio::task::block_in_place`] only works on a thread that is already a
//! worker of a multi-threaded runtime; it panics otherwise. Dossel's Guile
//! threads are plain OS threads created by `std::thread::spawn` and by libguile
//! — never Tokio workers — so `block_in_place` would panic on the first call.
//!
//! [`tokio::runtime::Handle::block_on`] is the supported way to drive a future
//! to completion *from outside* the runtime, which is exactly the situation
//! here. The future is polled on the calling Guile thread while the runtime's
//! own workers keep running elsewhere, so blocking here stalls only the REPL
//! session that made the call.

use std::future::Future;
use std::sync::OnceLock;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::api::FlorestaExtensionApi;
use crate::config::RuntimeConfig;
use crate::error::ApiError;
use crate::error::ApiResult;

/// Everything the Scheme primitives need in order to answer a call.
pub(crate) struct Attachment {
    api: Arc<dyn FlorestaExtensionApi>,
    runtime: Handle,
    config: RuntimeConfig,
}

impl Attachment {
    pub(crate) const fn config(&self) -> &RuntimeConfig {
        &self.config
    }
}

/// Set once, at node startup, before the REPL server begins accepting clients.
static ATTACHED: OnceLock<Attachment> = OnceLock::new();

/// Attach Dossel to a running node.
///
/// Returns `false` if an attachment already exists, which happens if a node is
/// started twice in one process (some integration tests do this). The first
/// attachment wins; the node handle is not swapped underneath live sessions.
pub(crate) fn attach(
    api: Arc<dyn FlorestaExtensionApi>,
    runtime: Handle,
    config: RuntimeConfig,
) -> bool {
    ATTACHED
        .set(Attachment {
            api,
            runtime,
            config,
        })
        .is_ok()
}

/// The current attachment, or [`ApiError::Detached`] if Dossel is not attached.
pub(crate) fn attachment() -> ApiResult<&'static Attachment> {
    ATTACHED.get().ok_or(ApiError::Detached)
}

/// Access the runtime configuration surface.
pub(crate) fn config() -> ApiResult<&'static RuntimeConfig> {
    attachment().map(Attachment::config)
}

/// Run an async node call from a Scheme primitive.
///
/// The shape every binding uses: look up the attachment, hand the API to
/// `f`, and block on the resulting future.
pub(crate) fn call<F, Fut, T>(f: F) -> ApiResult<T>
where
    F: FnOnce(&'static dyn FlorestaExtensionApi) -> Fut,
    Fut: Future<Output = ApiResult<T>>,
{
    let attachment = attachment()?;

    // `attachment` is `&'static`, so the API reference handed to `f` outlives
    // the future it builds.
    let api: &'static dyn FlorestaExtensionApi = attachment.api.as_ref();

    attachment.runtime.block_on(f(api))
}
