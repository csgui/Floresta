// SPDX-License-Identifier: MIT OR Apache-2.0

//! The boundary between Dossel and the node.
//!
//! Dossel deliberately does not depend on `floresta-wire`, `floresta-chain` or
//! `floresta-node`. It talks to the node exclusively through
//! [`FlorestaExtensionApi`], which the embedder implements. That keeps the
//! Guile/FFI machinery testable in isolation (see the mock in
//! [`crate::testing`]) and means Dossel can never reach past the surface the
//! embedder chose to expose.

use async_trait::async_trait;

use crate::error::ApiResult;

/// The node-side capabilities Dossel can surface to Scheme.
///
/// Implementations are shared across threads and called from the Guile threads,
/// so they must be `Send + Sync`. Calls are made through
/// [`crate::guile::bridge`], which drives them on the node's Tokio runtime.
#[async_trait]
pub trait FlorestaExtensionApi: Send + Sync + 'static {
    /// Height of the current best chain tip.
    async fn get_block_height(&self) -> ApiResult<u32>;

    /// Invoke a JSON-RPC method by name and return the parsed result.
    async fn rpc_call(
        &self,
        method: &str,
        params: Vec<serde_json::Value>,
    ) -> ApiResult<serde_json::Value>;
}
