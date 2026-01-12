// SPDX-License-Identifier: MIT

//! # Floresta Mempool
//!
//! This crate implements Floresta’s **policy-driven Bitcoin transaction mempool**.
//! It is responsible for managing unconfirmed transactions, enforcing non-consensus
//! admission and eviction policies, and providing services such as transaction relay
//! decisions, fee estimation, and block template construction.
//!
//! The mempool is **not part of consensus**. Instead, it builds on top of validated
//! chainstate abstractions and relies on an external implementation of
//! `BlockchainInterface` to query UTXO data, check transaction finality, and perform
//! script validation through consensus rules.
//!
//! The mempool is designed as an optional, composable service: nodes may operate
//! without a mempool (e.g., in headers-only or filter-only modes), while full nodes
//! and miners can integrate it to support transaction relay and block production.
//!
//! This crate contains no networking or persistence logic; it focuses solely on
//! in-memory transaction management and policy enforcement, allowing it to be reused
//! by higher-level components such as P2P networking layers, RPC servers, and mining
//! services.

pub mod mempool;

#[cfg(not(target_arch = "wasm32"))]
pub use mempool::Mempool;
pub use mempool::MempoolProof;
