//! Utreexo leaf data and hashing.
//!
//! This module defines the full leaf data structure used in the Utreexo accumulator
//! and the tagged hashing scheme for computing leaf hashes.
//!
//! # Overview
//!
//! Each UTXO in Bitcoin is represented as a leaf in the Utreexo accumulator. The leaf
//! contains all data needed to validate spending of the UTXO, plus additional commitments
//! that protect Utreexo-only nodes from certain attacks.
//!
//! # Leaf Hash Computation
//!
//! Leaf hashes use a tagged SHA-512/256 scheme (truncated to 256 bits) with domain separation.
//! The hash is computed as:
//!
//! ```text
//! leaf_hash = SHA-512/256(tag || tag || block_hash || txid || vout || header_code || utxo)
//! ```
//!
//! Where `tag` is [`UTREEXO_TAG_V1`], the SHA-512 hash of the string "UtreexoV1".
//! The tag is prepended twice following the BIP-340 tagged hash convention.
//!
//! # Security Properties
//!
//! The leaf data includes:
//! - **Block hash**: Commits to the block containing this UTXO, preventing block withholding attacks
//! - **Outpoint**: Uniquely identifies this UTXO
//! - **Header code**: Encodes block height and coinbase status for validation rules
//! - **TxOut**: The actual output (amount and scriptPubKey)
//!
//! See also: [`CompactLeafData`](super::CompactLeafData) for a bandwidth-efficient variant.

use bitcoin::consensus;
use bitcoin::consensus::Decodable;
use bitcoin::consensus::Encodable;
use bitcoin::hashes::sha256;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use bitcoin::OutPoint;
use bitcoin::TxOut;
use sha2::Digest;
use sha2::Sha512_256;

/// Domain separation tag for Utreexo V1 leaf hashes.
///
/// This is the SHA-512 hash of the ASCII string "UtreexoV1" (`[85, 116, 114, 101, 101, 120, 111, 86, 49]`
/// or `"5574726565786f5631"` in hex).
///
/// Following the BIP-340 tagged hash convention, this tag is prepended twice to the
/// data being hashed, providing domain separation from other hash uses.
pub const UTREEXO_TAG_V1: [u8; 64] = [
    0x5b, 0x83, 0x2d, 0xb8, 0xca, 0x26, 0xc2, 0x5b, 0xe1, 0xc5, 0x42, 0xd6, 0xcc, 0xed, 0xdd, 0xa8,
    0xc1, 0x45, 0x61, 0x5c, 0xff, 0x5c, 0x35, 0x72, 0x7f, 0xb3, 0x46, 0x26, 0x10, 0x80, 0x7e, 0x20,
    0xae, 0x53, 0x4d, 0xc3, 0xf6, 0x42, 0x99, 0x19, 0x99, 0x31, 0x77, 0x2e, 0x03, 0x78, 0x7d, 0x18,
    0x15, 0x6e, 0xb3, 0x15, 0x1e, 0x0e, 0xd1, 0xb3, 0x09, 0x8b, 0xdc, 0x84, 0x45, 0x86, 0x18, 0x85,
];

/// Full leaf data for the Utreexo accumulator.
///
/// This structure contains all the data that gets hashed to produce a leaf hash
/// in the Utreexo accumulator. It includes both the UTXO itself and metadata
/// needed for validation and security.
///
/// # Security Commitments
///
/// The leaf data binds several pieces of information together:
///
/// - **Block hash**: Prevents an attacker from claiming a UTXO exists in a different
///   block than it actually does. This is crucial for Utreexo nodes that don't store
///   the full UTXO set.
///
/// - **Header code**: Encodes the block height (for coinbase maturity checks) and
///   whether this is a coinbase output (which has special spending rules).
///
/// # Relationship to CompactLeafData
///
/// [`CompactLeafData`](super::CompactLeafData) is a bandwidth-optimized variant that
/// omits the block hash and outpoint (which can be derived from context) and uses
/// a recoverable scriptPubKey representation.
#[derive(Debug, PartialEq)]
pub struct LeafData {
    /// Hash of the block that created this UTXO.
    ///
    /// This commitment prevents attackers from providing fake proofs for UTXOs
    /// that don't exist or exist in different blocks.
    pub block_hash: BlockHash,
    /// The outpoint (txid:vout) identifying this UTXO.
    pub prevout: OutPoint,
    /// Compact encoding of block height and coinbase status.
    ///
    /// Encoding: `header_code = (block_height << 1) | is_coinbase`
    ///
    /// - Bit 0: Set to 1 if this UTXO was created by a coinbase transaction
    /// - Bits 1-31: The block height where this UTXO was created
    ///
    /// This is used for coinbase maturity validation (100-block rule).
    pub header_code: u32,
    /// The transaction output (amount and scriptPubKey).
    pub utxo: TxOut,
}

impl LeafData {
    /// Computes the leaf hash for this UTXO.
    ///
    /// The hash uniquely identifies this leaf in the Utreexo accumulator and commits
    /// to all the leaf data. It uses the tagged SHA-512/256 scheme defined by Utreexo V1.
    ///
    /// # Hash Computation
    ///
    /// The hash is computed as:
    /// ```text
    /// SHA-512/256(UTREEXO_TAG_V1 || UTREEXO_TAG_V1 || block_hash || txid || vout || header_code || utxo)
    /// ```
    ///
    /// All multi-byte integers are encoded in little-endian format. The `utxo` field
    /// is serialized using Bitcoin's consensus encoding.
    ///
    /// # Returns
    ///
    /// A 256-bit hash that serves as this leaf's identifier in the accumulator.
    pub fn _get_leaf_hashes(&self) -> sha256::Hash {
        let mut ser_utxo = Vec::new();
        self.utxo
            .consensus_encode(&mut ser_utxo)
            .expect("serializing TxOut never fails: Vec<u8>::Write always returns Ok");

        let leaf_hash = Sha512_256::new()
            .chain_update(UTREEXO_TAG_V1)
            .chain_update(UTREEXO_TAG_V1)
            .chain_update(self.block_hash)
            .chain_update(self.prevout.txid)
            .chain_update(self.prevout.vout.to_le_bytes())
            .chain_update(self.header_code.to_le_bytes())
            .chain_update(ser_utxo)
            .finalize();

        sha256::Hash::from_byte_array(leaf_hash.into())
    }
}

/// Implements Bitcoin consensus decoding for [`LeafData`].
///
/// The serialization format is:
/// ```text
/// [block_hash (32 bytes)][prevout (36 bytes)][header_code (4 bytes)][utxo (variable)]
/// ```
impl Decodable for LeafData {
    fn consensus_decode<R: bitcoin::io::Read + ?Sized>(
        reader: &mut R,
    ) -> Result<Self, consensus::encode::Error> {
        Self::consensus_decode_from_finite_reader(reader)
    }

    fn consensus_decode_from_finite_reader<R: bitcoin::io::Read + ?Sized>(
        reader: &mut R,
    ) -> Result<Self, consensus::encode::Error> {
        let block_hash = BlockHash::consensus_decode(reader)?;
        let prevout = OutPoint::consensus_decode(reader)?;
        let header_code = u32::consensus_decode(reader)?;
        let utxo = TxOut::consensus_decode(reader)?;
        Ok(LeafData {
            block_hash,
            prevout,
            header_code,
            utxo,
        })
    }
}
