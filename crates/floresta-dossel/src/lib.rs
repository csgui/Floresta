// SPDX-License-Identifier: MIT OR Apache-2.0

//! # Dossel — a programmable runtime extension layer for Floresta
//!
//! Dossel embeds a GNU Guile Scheme interpreter in a running Floresta node and
//! serves a REPL over a Unix domain socket. An operator connects, inspects the
//! node's state, and disconnects; the node runs throughout.
//!
//! ```text
//! $ floresta-repl
//! dossel> (get-block-height)
//! $1 = 840443
//! dossel> (quit)
//! ; session ends, node keeps running
//! ```
//!
//! This is a minimal, deliberately small starting point — one procedure,
//! rebuilt from a from-scratch redesign — not a fixed API surface. New
//! capabilities get added to [`api::FlorestaExtensionApi`] and `node.scm` one
//! at a time, as they are actually needed.
//!
//! ## Structure
//!
//! * [`api`] — [`FlorestaExtensionApi`], the trait the embedder implements.
//!   Dossel depends on no other Floresta crate, so the whole FFI layer is
//!   testable against a mock.
//! * [`guile`] — the interpreter: raw bindings, safe wrappers, the
//!   `(floresta node)` module, the async bridge, the REPL server and the thread
//!   that owns them.
//!
//! ## Sessions share one environment
//!
//! Every REPL client evaluates in the same `(guile-user)` module, which is
//! Guile's own REPL-server behaviour. A definition made in one session is
//! visible in all the others, including sessions opened later — convenient for
//! an operator building up helpers across a debugging session, but it also
//! means one session can shadow a procedure another is relying on. `--load` is
//! the same environment, populated before any client connects.
//!
//! ## Security
//!
//! Anything that can connect to the REPL socket can evaluate arbitrary Scheme
//! inside the node process. The socket is created `0600` inside a `0700`
//! directory, and Dossel never opens a TCP listener. Treat access to the socket
//! as equivalent to access to the node's user account.
//!
//! ## Consensus is not here
//!
//! Block validation, difficulty adjustment and signature verification have no
//! representation in this crate — not as writable state, not as readable state,
//! not indirectly. [`FlorestaExtensionApi`] is the complete surface, and none of
//! its methods touch consensus.

pub mod api;
pub mod error;
mod guile;
pub mod testing;

use std::sync::Arc;

use tokio::runtime::Handle;

pub use crate::api::FlorestaExtensionApi;
pub use crate::error::ApiError;
pub use crate::error::ApiResult;
pub use crate::error::DosselError;
pub use crate::guile::runtime::DosselConfig;
pub use crate::guile::runtime::DosselRuntime;

impl DosselRuntime {
    /// Attach Dossel to a running node and start the REPL.
    ///
    /// Must be called from within a Tokio runtime: the Guile threads drive
    /// node calls on that runtime via [`Handle::block_on`].
    ///
    /// Only the first call in a process takes effect. A second call leaves the
    /// existing attachment in place rather than swapping the node handle out
    /// from under live REPL sessions, and logs a warning.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no ambient Tokio runtime, or if the socket
    /// path cannot be prepared. Failures *after* the thread starts — a socket
    /// that will not bind, a broken init file — are logged by the Guile thread
    /// instead, so that nothing Dossel does can abort node startup.
    pub fn spawn(config: DosselConfig, api: Arc<dyn FlorestaExtensionApi>) -> Result<Self, DosselError> {
        let handle = Handle::try_current().map_err(|_| DosselError::NoTokioRuntime)?;

        if !guile::bridge::attach(api, handle) {
            tracing::warn!(
                "Dossel is already attached to a node in this process; \
                 keeping the existing attachment"
            );
        }

        Self::start(config)
    }
}
