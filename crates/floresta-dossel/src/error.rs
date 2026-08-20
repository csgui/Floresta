// SPDX-License-Identifier: MIT OR Apache-2.0

//! Error types for Dossel.
//!
//! Errors fall into two groups. [`DosselError`] covers failures in setting up
//! and running the extension layer itself (Guile could not start, the socket
//! could not be bound). [`ApiError`] covers failures of an individual call into
//! the node, and is what a REPL user sees as a Scheme `misc-error`.

use std::path::PathBuf;

/// A failure in the Dossel runtime itself.
#[derive(Debug, thiserror::Error)]
pub enum DosselError {
    /// The REPL socket path could not be prepared.
    #[error("could not prepare REPL socket directory {path}: {source}")]
    SocketDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A stale socket file could not be removed before binding.
    #[error("could not remove stale REPL socket {path}: {source}")]
    StaleSocket {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The socket path contains bytes that cannot be passed to Guile.
    #[error("REPL socket path {0} is not valid UTF-8")]
    NonUtf8SocketPath(PathBuf),

    /// A Unix domain socket path is longer than the platform's `sun_path`.
    #[error(
        "REPL socket path {path} is {len} bytes, which exceeds the {max}-byte limit for Unix \
         domain sockets; choose a shorter datadir or set the socket path explicitly"
    )]
    SocketPathTooLong {
        path: PathBuf,
        len: usize,
        max: usize,
    },

    /// `DosselRuntime::spawn` was called outside a Tokio runtime.
    #[error(
        "Dossel must be started from within a Tokio runtime; the Guile threads drive node \
         calls on it"
    )]
    NoTokioRuntime,

    /// The OS refused to spawn the Guile thread.
    #[error("could not spawn the Dossel Guile thread: {0}")]
    ThreadSpawn(#[source] std::io::Error),

    /// Scheme evaluated during startup raised an error.
    #[error("Dossel startup failed while evaluating {context}: {message}")]
    Startup { context: String, message: String },

    /// A string handed to Guile contained an interior NUL byte.
    #[error("cannot pass {context} to Guile: it contains an interior NUL byte")]
    InteriorNul { context: String },
}

/// A failure of a single call from Scheme into the node.
///
/// Every variant is rendered into the message of a Scheme `misc-error`
/// condition, attributed to the procedure the operator called, so the text is
/// user-facing. Keep it actionable.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// The requested capability is not implemented by this build of Floresta.
    ///
    /// This is not a bug and not a stub left behind by accident. Several
    /// bindings in the Phase 1 API surface describe node features that Floresta
    /// does not currently have (see the crate-level docs). They are registered
    /// so that the module's shape is stable, and they say plainly why they
    /// cannot answer.
    #[error("{capability} is not available: {reason}")]
    Unsupported {
        capability: &'static str,
        reason: &'static str,
    },

    /// The node was asked for something that does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// An argument from Scheme was the wrong shape or out of range.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The node task did not answer. Usually means it is shutting down.
    #[error("the node did not respond: {0}")]
    NodeUnreachable(String),

    /// The node answered with a failure.
    #[error("{0}")]
    Node(String),

    /// Dossel has not been given a node handle yet.
    #[error("Dossel is not attached to a running node")]
    Detached,
}

impl ApiError {
    /// Convenience constructor for a capability Floresta does not implement.
    pub const fn unsupported(capability: &'static str, reason: &'static str) -> Self {
        Self::Unsupported { capability, reason }
    }
}

/// Result alias for calls made from Scheme into the node.
pub type ApiResult<T> = Result<T, ApiError>;
