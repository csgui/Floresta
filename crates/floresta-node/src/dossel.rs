// SPDX-License-Identifier: MIT OR Apache-2.0

//! The bridge between Dossel and this node.
//!
//! [`floresta_dossel`] deliberately knows nothing about Floresta's types. This
//! module supplies the other half: an implementation of
//! [`FlorestaExtensionApi`] over the chain state and the JSON-RPC dispatcher,
//! plus the [`RuntimeConfig`] bindings for the configuration keys this build
//! can actually serve.
//!
//! # What is bound, and what is not
//!
//! Three configuration keys are bound read-only: `network`, `datadir` and
//! `version` are fixed for the life of the process, and `ban-threshold` reads
//! the live `max_banscore` but cannot change it — Floresta has no request
//! that would let it. `log-level` is bound read-write when the embedding
//! binary supplies a [`LogLevelControl`], as `florestad` does.
//!
//! Every other key stays unbound, which makes `(get-config …)` and
//! `(set-config! …)` report the concrete reason rather than a plausible lie.
//! See the table in [`floresta_dossel`]'s crate documentation.

use std::path::PathBuf;
use std::sync::Arc;

use bitcoin::Network;
use floresta_chain::ThreadSafeChain;
use floresta_dossel::ApiError;
use floresta_dossel::ApiResult;
use floresta_dossel::ConfigKey;
use floresta_dossel::ConfigValue;
use floresta_dossel::FlorestaExtensionApi;
use floresta_dossel::RuntimeConfig;
use floresta_dossel::config::FnBackend;
use floresta_dossel::config::StaticBackend;
use floresta_wire::node_handle::NodeHandle;
use floresta_wire::node_interface::NodeConfigMethods;
use tokio::runtime::Handle;

#[cfg(feature = "json-rpc")]
use crate::json_rpc::request::RpcRequest;
#[cfg(feature = "json-rpc")]
use crate::json_rpc::server::RpcImpl;

use crate::florestad::LogLevelControl;

/// Map a `RecvError` from the node channel onto an API error.
///
/// The node handle answers over a oneshot channel; a receive failure means the
/// node task dropped the sender, which in practice means it is shutting down.
fn node_gone(e: impl std::fmt::Display) -> ApiError {
    ApiError::NodeUnreachable(e.to_string())
}

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

/// Build the configuration surface Dossel exposes for this node.
///
/// Deliberately sparse. A key is bound only when this build can genuinely
/// answer for it; the rest report why they cannot.
///
/// # Panics
///
/// Must be called from within a Tokio runtime: the `ban-threshold` backend
/// captures the current [`Handle`] so it can reach the node task later, from a
/// Guile thread that has no ambient runtime of its own.
pub fn runtime_config(
    node: NodeHandle,
    network: Network,
    datadir: PathBuf,
    log_level_control: Option<LogLevelControl>,
) -> RuntimeConfig {
    let config = RuntimeConfig::new();

    // Captured here, while we are still on a runtime thread. Looking it up
    // inside the closure would fail: config backends run on Guile threads,
    // which are not Tokio workers and have no runtime in scope.
    let handle = Handle::current();

    config.bind(
        ConfigKey::Network,
        Arc::new(StaticBackend::new(ConfigValue::Symbol(
            network_name(network).to_owned(),
        ))),
    );

    config.bind(
        ConfigKey::Datadir,
        Arc::new(StaticBackend::new(ConfigValue::Str(
            datadir.display().to_string(),
        ))),
    );

    config.bind(
        ConfigKey::Version,
        Arc::new(StaticBackend::new(ConfigValue::Str(
            env!("CARGO_PKG_VERSION").to_owned(),
        ))),
    );

    // Readable but not writable: `max_banscore` lives inside the node task's
    // own state and there is no request that would change it.
    config.bind(
        ConfigKey::BanThreshold,
        Arc::new(FnBackend::read_only(move || {
            // Blocks the calling REPL session only: the Guile thread is not a
            // Tokio worker, so nothing on the runtime waits on this. See
            // `floresta_dossel::guile::bridge` for why it is `block_on` and not
            // `block_in_place`.
            let node = node.clone();
            let config = handle
                .block_on(async move { node.get_config().await })
                .map_err(node_gone)?;

            Ok(ConfigValue::Integer(i128::from(config.max_banscore)))
        })),
    );

    // Read-write when the embedding binary supplied a log-level control:
    // the closures already know how to swap the live tracing filter.
    if let Some(control) = log_level_control {
        let get = control.get;
        let set = control.set;
        config.bind(
            ConfigKey::LogLevel,
            Arc::new(FnBackend::read_write(
                move || Ok(ConfigValue::Str(get())),
                move |value| {
                    let spec = match value {
                        ConfigValue::Str(s) | ConfigValue::Symbol(s) => s,
                        other => {
                            return Err(ApiError::Node(format!(
                                "log-level expects a string like \"debug\" or \
                                 \"info,wire=trace\", got {}",
                                other.type_name()
                            )));
                        }
                    };
                    set(&spec).map_err(ApiError::Node)
                },
            )),
        );
    }

    config
}

/// The symbol name for a network, matching what `--network` accepts.
const fn network_name(network: Network) -> &'static str {
    match network {
        Network::Bitcoin => "mainnet",
        Network::Testnet4 => "testnet4",
        Network::Signet => "signet",
        Network::Regtest => "regtest",
        _ => "testnet",
    }
}
