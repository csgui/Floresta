// SPDX-License-Identifier: MIT OR Apache-2.0

//! The bridge between Dossel and this node.
//!
//! [`floresta_dossel`] deliberately knows nothing about Floresta's types. This
//! module supplies the other half: an implementation of
//! [`FlorestaExtensionApi`] over the chain state and the JSON-RPC dispatcher.

use std::sync::Arc;

use floresta_chain::ThreadSafeChain;
use floresta_dossel::ApiError;
use floresta_dossel::ApiResult;
use floresta_dossel::FlorestaExtensionApi;

#[cfg(feature = "json-rpc")]
use crate::json_rpc::request::RpcRequest;
#[cfg(feature = "json-rpc")]
use crate::json_rpc::server::RpcImpl;

/// Dossel's view of this node.
pub struct NodeExtensionApi<Chain>
where
    Chain: ThreadSafeChain + Clone,
{
    chain: Chain,

    #[cfg(feature = "json-rpc")]
    rpc: Arc<RpcImpl<Chain>>,
}

impl<Chain> NodeExtensionApi<Chain>
where
    Chain: ThreadSafeChain + Clone,
{
    #[cfg(feature = "json-rpc")]
    pub fn new(chain: Chain, rpc: Arc<RpcImpl<Chain>>) -> Self {
        Self { chain, rpc }
    }

    #[cfg(not(feature = "json-rpc"))]
    pub fn new(chain: Chain) -> Self {
        Self { chain }
    }
}

#[async_trait::async_trait]
impl<Chain> FlorestaExtensionApi for NodeExtensionApi<Chain>
where
    Chain: ThreadSafeChain + Clone + 'static,
{
    async fn get_block_height(&self) -> ApiResult<u32> {
        // Chain reads are synchronous: `BlockchainInterface` is backed by an
        // in-process lock, not by the node task. No round trip is involved.
        self.chain
            .get_height()
            .map_err(|e| ApiError::Node(e.to_string()))
    }

    #[cfg(feature = "json-rpc")]
    async fn rpc_call(
        &self,
        method: &str,
        params: Vec<serde_json::Value>,
    ) -> ApiResult<serde_json::Value> {
        let request = RpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            method: method.to_owned(),
            params: Some(serde_json::Value::Array(params)),
            id: serde_json::Value::String("dossel".to_owned()),
        };

        Arc::clone(&self.rpc)
            .dispatch(request)
            .await
            .map_err(|e| {
                // `JsonRpcError` has no `Display`; `rpc_error` is the rendering
                // the HTTP server itself sends to clients, so the REPL sees the
                // same wording as any other RPC caller.
                let rendered = e.rpc_error();
                match rendered.data {
                    Some(data) => ApiError::Node(format!("{} ({data})", rendered.message)),
                    None => ApiError::Node(rendered.message),
                }
            })
    }

    #[cfg(not(feature = "json-rpc"))]
    async fn rpc_call(
        &self,
        _method: &str,
        _params: Vec<serde_json::Value>,
    ) -> ApiResult<serde_json::Value> {
        Err(ApiError::unsupported(
            "rpc-call",
            "this florestad was built without the json-rpc feature, so there is no RPC \
             dispatcher to call into",
        ))
    }
}
