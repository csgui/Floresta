// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mock [`FlorestaExtensionApi`] for tests.
//!
//! Dossel's FFI layer is the part most worth testing and the part least
//! convenient to test against a real node — a real node needs a chain, peers
//! and a network. [`MockApi`] stands in for one, so the Guile bindings can be
//! exercised without any of that.
//!
//! This is deliberately part of the public API: an embedder writing tests for
//! its own [`FlorestaExtensionApi`] implementation will want the same thing.

use async_trait::async_trait;

use crate::api::FlorestaExtensionApi;
use crate::error::ApiResult;

/// A [`FlorestaExtensionApi`] that answers from fixed data.
#[derive(Debug, Clone)]
pub struct MockApi {
    pub height: u32,
}

impl Default for MockApi {
    fn default() -> Self {
        Self { height: 840_443 }
    }
}

#[async_trait]
impl FlorestaExtensionApi for MockApi {
    async fn get_block_height(&self) -> ApiResult<u32> {
        Ok(self.height)
    }

    async fn rpc_call(
        &self,
        method: &str,
        params: Vec<serde_json::Value>,
    ) -> ApiResult<serde_json::Value> {
        Ok(serde_json::json!({ "method": method, "params": params }))
    }
}
