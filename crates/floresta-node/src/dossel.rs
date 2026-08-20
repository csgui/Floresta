// SPDX-License-Identifier: MIT OR Apache-2.0

//! The bridge between Dossel and this node.
//!
//! [`floresta_dossel`] deliberately knows nothing about Floresta's types. This
//! module supplies the other half: an implementation of
//! [`FlorestaExtensionApi`] over the chain state.

use floresta_chain::ThreadSafeChain;
use floresta_dossel::ApiError;
use floresta_dossel::ApiResult;
use floresta_dossel::FlorestaExtensionApi;

/// Dossel's view of this node.
pub struct NodeExtensionApi<Chain>
where
    Chain: ThreadSafeChain + Clone,
{
    chain: Chain,
}

impl<Chain> NodeExtensionApi<Chain>
where
    Chain: ThreadSafeChain + Clone,
{
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
}
