// SPDX-License-Identifier: MIT

//! Core domain types for Floresta's Utreexo implementation.
//!
//! This crate provides the fundamental data structures and abstractions used across
//! [Floresta](https://github.com/getfloresta/floresta) for working with
//! [Utreexo](https://eprint.iacr.org/2019/611.pdf), a compact cryptographic accumulator
//! for the Bitcoin UTXO set.
//!
//! # Overview
//!
//! Utreexo allows Bitcoin nodes to verify transactions without storing the entire UTXO set.
//! Instead of maintaining millions of unspent outputs, nodes keep only a small set of
//! cryptographic hashes (the accumulator roots) and verify inclusion proofs for each
//! transaction input.
//!
//! This crate defines the core types needed for Utreexo operations:
//!
//! - **[`LeafData`](utreexo::LeafData)**: Full UTXO data hashed into accumulator leaves
//! - **[`CompactLeafData`](utreexo::CompactLeafData)**: Bandwidth-efficient representation for proof propagation
//! - **[`ScriptPubKeyKind`](utreexo::ScriptPubKeyKind)**: Script type indicator for scriptPubKey recovery
//! - **[`UTREEXO_TAG_V1`](utreexo::UTREEXO_TAG_V1)**: Domain separation tag for leaf hashing

// cargo docs customization
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_logo_url = "https://avatars.githubusercontent.com/u/249173822")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/getfloresta/floresta-media/master/logo_png/Icon-Green(main).png"
)]

pub mod mempool;
pub mod utreexo;
