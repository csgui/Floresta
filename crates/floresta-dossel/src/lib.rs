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
//! dossel> (get-config 'network)
//! $2 = regtest
//! dossel> (quit)
//! ; session ends, node keeps running
//! ```
//!
//! This surface is grown one procedure at a time from a from-scratch
//! redesign, not delivered as a fixed API up front: `get-block-height`,
//! `rpc-call`, `get-config` and `set-config!` so far. New capabilities get
//! added to [`api::FlorestaExtensionApi`] and `node.scm` as they are actually
//! needed.
//!
//! ## Structure
//!
//! * [`api`] — [`FlorestaExtensionApi`], the trait the embedder implements.
//!   Dossel depends on no other Floresta crate, so the whole FFI layer is
//!   testable against a mock.
//! * [`config`] — the closed set of runtime configuration keys.
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
//! not indirectly through `rpc-call` or `set-config!`. [`FlorestaExtensionApi`]
//! and [`config::ConfigKey`] are the complete surface, and none of it touches
//! consensus.
//!
//! ## What this build cannot do
//!
//! [`config::ConfigKey`]'s list is fixed regardless of build, so that
//! `(get-config …)`/`(set-config! …)`'s set of valid keys never varies —
//! three of them are not bound to anything by `florestad` today, and say so
//! rather than accepting a write that does nothing:
//!
//! | Key | Why it is unavailable |
//! |---|---|
//! | `(set-config! 'max-peers …)` | the peer limit is `NodeContext::MAX_OUTGOING_PEERS`, an associated const fixed at compile time |
//! | `(set-config! 'mempool-max-size-mb …)` | fixed when the node task builds its mempool |
//! | `(set-config! 'fee-filter-rate …)` | Floresta has no fee filter rate |
//!
//! Each becomes available the moment an embedder binds a
//! [`config::ConfigBackend`] for it. No change to the Scheme layer is needed.

pub mod api;
pub mod config;
pub mod error;
mod guile;
pub mod testing;

use std::sync::Arc;

use tokio::runtime::Handle;

pub use crate::api::FlorestaExtensionApi;
pub use crate::config::ConfigKey;
pub use crate::config::ConfigValue;
pub use crate::config::RuntimeConfig;
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
    pub fn spawn(
        config: DosselConfig,
        api: Arc<dyn FlorestaExtensionApi>,
        runtime_config: RuntimeConfig,
    ) -> Result<Self, DosselError> {
        let handle = Handle::try_current().map_err(|_| DosselError::NoTokioRuntime)?;

        if !guile::bridge::attach(api, handle, runtime_config) {
            tracing::warn!(
                "Dossel is already attached to a node in this process; \
                 keeping the existing attachment"
            );
        }

        Self::start(config)
    }
}
