//! Compact leaf data types for Utreexo proofs.
//!
//! This module provides compact representations of UTXO leaf data used in Utreexo
//! accumulator proofs. The compact format reduces bandwidth by avoiding redundant
//! data that can be recovered from spending transactions.
//!
//! # Overview
//!
//! When propagating Utreexo proofs, we need to transmit leaf data for each spent UTXO.
//! Rather than sending the full scriptPubKey, we can often recover it from the spending
//! transaction's scriptSig or witness data. This module defines:
//!
//! - [`CompactLeafData`]: A bandwidth-efficient representation of UTXO metadata
//! - [`ScriptPubKeyKind`]: An enum indicating how to recover the scriptPubKey
//!
//! # Serialization Format
//!
//! [`ScriptPubKeyKind`] uses a single-byte type prefix:
//!
//! | Byte | Type                    | Recovery Method                           |
//! |------|-------------------------|-------------------------------------------|
//! | 0x00 | Other                   | Raw script follows (no recovery possible) |
//! | 0x01 | P2PKH                   | Hash the pubkey from scriptSig            |
//! | 0x02 | P2WPKH                  | Hash the pubkey from witness              |
//! | 0x03 | P2SH                    | Hash the redeem script from scriptSig     |
//! | 0x04 | P2WSH                   | Hash the witness script from witness      |

use bitcoin::consensus;
use bitcoin::consensus::Decodable;
use bitcoin::consensus::Encodable;

/// Compact representation of UTXO leaf data for Utreexo proofs.
///
/// This struct contains the minimal information needed to reconstruct and verify
/// a UTXO leaf in the Utreexo accumulator. It's designed to minimize bandwidth
/// when propagating proofs between nodes.
///
/// # Serialization Format
///
/// The serialized format is: `[<header_code><amount><spk_type>]`
///
/// # Header Code Encoding
///
/// The `header_code` field compactly encodes two pieces of information:
/// - **Bit 0**: Set to 1 if the UTXO was created by a coinbase transaction
/// - **Bits 1-31**: The block height where this UTXO was created
///
/// ```text
/// header_code = (block_height << 1) | (is_coinbase ? 1 : 0)
/// ```
///
/// To decode:
/// - `is_coinbase = (header_code & 1) != 0`
/// - `block_height = header_code >> 1`
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct CompactLeafData {
    /// Encodes the creation block height (bits 1-31) and coinbase flag (bit 0).
    ///
    /// Use `header_code >> 1` to get the height and `header_code & 1` for the coinbase flag.
    pub header_code: u32,
    /// The amount in satoshis locked in this UTXO.
    pub amount: u64,
    /// The scriptPubKey type, used to reconstruct the full script from the spending tx.
    pub spk_ty: ScriptPubKeyKind,
}

/// Indicates the scriptPubKey type for bandwidth-efficient script recovery.
///
/// Instead of transmitting the full scriptPubKey, we send a type indicator and
/// recover the script from the spending transaction. This works because standard
/// script types require the spender to reveal the preimage (pubkey or script)
/// that hashes to the scriptPubKey.
///
/// # Security
///
/// This optimization is safe because the full leaf data (including the recovered
/// scriptPubKey) is committed in the Utreexo leaf hash. Any attempt to provide
/// incorrect recovery data would result in a hash mismatch.
///
/// # Example
///
/// For P2PKH, the scriptPubKey is `OP_DUP OP_HASH160 <pubkey_hash> OP_EQUALVERIFY OP_CHECKSIG`.
/// The spending scriptSig contains the public key. We can hash it to get `<pubkey_hash>`
/// and reconstruct the full scriptPubKey without transmitting it.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ScriptPubKeyKind {
    /// Non-standard script type; the raw script bytes are included.
    ///
    /// Used when the script cannot be recovered from the spending transaction.
    Other(Box<[u8]>),
    /// Pay-to-Public-Key-Hash (P2PKH).
    ///
    /// Recovered by hashing the public key from the scriptSig.
    PubKeyHash,
    /// Pay-to-Witness-Public-Key-Hash (P2WPKH).
    ///
    /// Recovered by hashing the public key from the witness stack.
    WitnessV0PubKeyHash,
    /// Pay-to-Script-Hash (P2SH).
    ///
    /// Recovered by hashing the redeem script from the scriptSig.
    ScriptHash,
    /// Pay-to-Witness-Script-Hash (P2WSH).
    ///
    /// Recovered by hashing the witness script from the witness stack.
    WitnessV0ScriptHash,
}

impl Decodable for ScriptPubKeyKind {
    fn consensus_decode<R: bitcoin::io::Read + ?Sized>(
        reader: &mut R,
    ) -> Result<Self, consensus::encode::Error> {
        let ty = u8::consensus_decode(reader)?;
        match ty {
            0x00 => Ok(ScriptPubKeyKind::Other(Box::consensus_decode(reader)?)),
            0x01 => Ok(ScriptPubKeyKind::PubKeyHash),
            0x02 => Ok(ScriptPubKeyKind::WitnessV0PubKeyHash),
            0x03 => Ok(ScriptPubKeyKind::ScriptHash),
            0x04 => Ok(ScriptPubKeyKind::WitnessV0ScriptHash),
            _ => Err(consensus::encode::Error::ParseFailed("Invalid script type")),
        }
    }
}

impl Encodable for ScriptPubKeyKind {
    fn consensus_encode<W: bitcoin::io::Write + ?Sized>(
        &self,
        writer: &mut W,
    ) -> Result<usize, bitcoin::io::Error> {
        let mut len = 1;

        match self {
            ScriptPubKeyKind::Other(script) => {
                00_u8.consensus_encode(writer)?;
                len += script.consensus_encode(writer)?;
            }
            ScriptPubKeyKind::PubKeyHash => {
                0x01_u8.consensus_encode(writer)?;
            }
            ScriptPubKeyKind::WitnessV0PubKeyHash => {
                0x02_u8.consensus_encode(writer)?;
            }
            ScriptPubKeyKind::ScriptHash => {
                0x03_u8.consensus_encode(writer)?;
            }
            ScriptPubKeyKind::WitnessV0ScriptHash => {
                0x04_u8.consensus_encode(writer)?;
            }
        }
        Ok(len)
    }
}
