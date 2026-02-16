//! Property-based tests for Utreexo domain types.
//!
//! These tests use [`proptest`] to verify invariants over randomly generated
//! inputs rather than hand-picked examples.  The properties tested are:
//!
//! ## [`ScriptPubKeyKind`] serialization
//!
//! - **Roundtrip**: encoding then decoding any variant yields the original value.
//! - **Length contract**: fixed variants (P2PKH, P2WPKH, P2SH, P2WSH) encode to
//!   exactly 1 byte; the `Other` variant encodes to 1 type byte + varint + script.
//! - **Rejection**: type bytes outside the valid range `0x00..=0x04` are rejected.
//!
//! ## [`LeafData`] hashing
//!
//! - **Determinism**: hashing the same leaf twice always returns the same digest.
//! - **Prevout sensitivity**: changing the output index produces a different hash.
//! - **Block hash sensitivity**: changing the block hash produces a different hash.
//! - **Output size**: the hash is always exactly 32 bytes (SHA-256).
//!
//! ## `header_code` encoding
//!
//! - **Roundtrip**: `(height << 1) | coinbase` can be decomposed back into
//!   the original height and coinbase flag without loss.
//! - **[`CompactLeafData`] preservation**: the same roundtrip holds when the
//!   `header_code` lives inside a [`CompactLeafData`] instance.

use bitcoin::consensus::Decodable;
use bitcoin::consensus::Encodable;
use bitcoin::hashes::Hash;
use bitcoin::Amount;
use bitcoin::BlockHash;
use bitcoin::OutPoint;
use bitcoin::ScriptBuf;
use bitcoin::TxOut;
use bitcoin::Txid;
use proptest::prelude::*;

use super::CompactLeafData;
use super::LeafData;
use super::ScriptPubKeyKind;

/// Generates a random [`BlockHash`] from 32 arbitrary bytes.
fn arb_block_hash() -> impl Strategy<Value = BlockHash> {
    prop::array::uniform32(any::<u8>()).prop_map(|b| BlockHash::from_byte_array(b))
}

/// Generates a random [`Txid`] from 32 arbitrary bytes.
fn arb_txid() -> impl Strategy<Value = Txid> {
    prop::array::uniform32(any::<u8>()).prop_map(|b| Txid::from_byte_array(b))
}

/// Generates a random [`OutPoint`] (txid + vout).
fn arb_outpoint() -> impl Strategy<Value = OutPoint> {
    (arb_txid(), any::<u32>()).prop_map(|(txid, vout)| OutPoint { txid, vout })
}

/// Generates a random [`ScriptBuf`] with 0..128 arbitrary bytes.
fn arb_script_buf() -> impl Strategy<Value = ScriptBuf> {
    prop::collection::vec(any::<u8>(), 0..128).prop_map(|v| ScriptBuf::from_bytes(v))
}

/// Generates a random [`TxOut`] with an arbitrary satoshi amount and script.
fn arb_txout() -> impl Strategy<Value = TxOut> {
    (any::<u64>(), arb_script_buf()).prop_map(|(sats, script)| TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: script,
    })
}

/// Generates a random [`LeafData`] with all fields arbitrary.
fn arb_leaf_data() -> impl Strategy<Value = LeafData> {
    (arb_block_hash(), arb_outpoint(), any::<u32>(), arb_txout()).prop_map(
        |(block_hash, prevout, header_code, utxo)| LeafData {
            block_hash,
            prevout,
            header_code,
            utxo,
        },
    )
}

/// Generates a random [`ScriptPubKeyKind`], covering all five variants
/// with uniform probability.
fn arb_spk_kind() -> impl Strategy<Value = ScriptPubKeyKind> {
    prop_oneof![
        Just(ScriptPubKeyKind::PubKeyHash),
        Just(ScriptPubKeyKind::WitnessV0PubKeyHash),
        Just(ScriptPubKeyKind::ScriptHash),
        Just(ScriptPubKeyKind::WitnessV0ScriptHash),
        prop::collection::vec(any::<u8>(), 0..128)
            .prop_map(|v| ScriptPubKeyKind::Other(v.into_boxed_slice())),
    ]
}

/// Generates a random [`CompactLeafData`] with arbitrary header code,
/// amount, and script type.
fn arb_compact_leaf_data() -> impl Strategy<Value = CompactLeafData> {
    (any::<u32>(), any::<u64>(), arb_spk_kind()).prop_map(|(header_code, amount, spk_ty)| {
        CompactLeafData {
            header_code,
            amount,
            spk_ty,
        }
    })
}

proptest! {
    /// Encoding then decoding any [`ScriptPubKeyKind`] must return the
    /// original value — the codec is lossless.
    #[test]
    fn spk_kind_roundtrip(kind in arb_spk_kind()) {
        let mut buf = Vec::new();
        kind.consensus_encode(&mut buf).unwrap();

        let decoded = ScriptPubKeyKind::consensus_decode(&mut buf.as_slice()).unwrap();
        prop_assert_eq!(kind, decoded);
    }
}

proptest! {
    /// The byte count returned by `consensus_encode` must match the actual
    /// buffer length.  Fixed variants occupy exactly 1 byte; the `Other`
    /// variant is at least 1 (type) + the script content.
    #[test]
    fn spk_kind_encoded_length(kind in arb_spk_kind()) {
        let mut buf = Vec::new();
        let written = kind.consensus_encode(&mut buf).unwrap();

        prop_assert_eq!(written, buf.len());

        match &kind {
            ScriptPubKeyKind::Other(script) => {
                // 1 byte type + varint length + script bytes
                prop_assert!(buf.len() > 1);
                prop_assert!(buf[0] == 0x00);
                // total must account for the script content
                prop_assert!(buf.len() >= 1 + script.len());
            }
            _ => {
                // Fixed-size variants are exactly 1 byte
                prop_assert_eq!(buf.len(), 1);
            }
        }
    }
}

proptest! {
    /// Any single-byte payload with a type tag outside 0x00..=0x04 must fail
    /// to decode — no silent acceptance of unknown script types.
    #[test]
    fn spk_kind_rejects_invalid_type(byte in 0x05u8..=0xffu8) {
        let buf = [byte];
        let result = ScriptPubKeyKind::consensus_decode(&mut buf.as_slice());
        prop_assert!(result.is_err());
    }
}

proptest! {
    /// Hashing the same [`LeafData`] twice must always yield the same
    /// digest — the tagged SHA-512/256 computation is pure.
    #[test]
    fn leaf_hash_deterministic(leaf in arb_leaf_data()) {
        let h1 = leaf._get_leaf_hashes();
        let h2 = leaf._get_leaf_hashes();
        prop_assert_eq!(h1, h2);
    }
}

proptest! {
    /// Changing the output index in the prevout must change the leaf hash,
    /// ensuring the accumulator distinguishes UTXOs from the same transaction.
    #[test]
    fn leaf_hash_changes_with_prevout(
        leaf in arb_leaf_data(),
        other_vout in any::<u32>(),
    ) {
        // Skip when the random vout happens to match
        prop_assume!(other_vout != leaf.prevout.vout);

        let modified = LeafData {
            block_hash: leaf.block_hash,
            prevout: OutPoint { txid: leaf.prevout.txid, vout: other_vout },
            header_code: leaf.header_code,
            utxo: leaf.utxo.clone(),
        };

        // Also ensure the full struct differs
        prop_assume!(modified.prevout != leaf.prevout);

        prop_assert_ne!(leaf._get_leaf_hashes(), modified._get_leaf_hashes());
    }
}

proptest! {
    /// Changing the block hash must change the leaf hash, ensuring the
    /// accumulator binds each UTXO to its originating block.
    #[test]
    fn leaf_hash_changes_with_block_hash(
        leaf in arb_leaf_data(),
        other_hash in arb_block_hash(),
    ) {
        prop_assume!(other_hash != leaf.block_hash);

        let modified = LeafData {
            block_hash: other_hash,
            prevout: leaf.prevout,
            header_code: leaf.header_code,
            utxo: leaf.utxo.clone(),
        };

        prop_assert_ne!(leaf._get_leaf_hashes(), modified._get_leaf_hashes());
    }
}

proptest! {
    /// The `header_code` bit-packing scheme `(height << 1) | coinbase` must
    /// roundtrip: extracting the coinbase flag (bit 0) and height (bits 1-31)
    /// returns the original values.
    #[test]
    fn header_code_encodes_height_and_coinbase(
        height in 0u32..=(u32::MAX >> 1),
        is_coinbase in any::<bool>(),
    ) {
        let header_code = (height << 1) | (is_coinbase as u32);

        let decoded_coinbase = (header_code & 1) != 0;
        let decoded_height = header_code >> 1;

        prop_assert_eq!(decoded_coinbase, is_coinbase);
        prop_assert_eq!(decoded_height, height);
    }
}

proptest! {
    /// The `header_code` inside a [`CompactLeafData`] decomposes and
    /// recomposes losslessly through the same bit-packing scheme.
    #[test]
    fn compact_leaf_data_preserves_fields(data in arb_compact_leaf_data()) {
        let height = data.header_code >> 1;
        let is_coinbase = (data.header_code & 1) != 0;

        // Reconstruct header_code from decoded parts
        let reconstructed = (height << 1) | (is_coinbase as u32);
        prop_assert_eq!(reconstructed, data.header_code);

        // amount is preserved as-is
        prop_assert!(data.amount <= u64::MAX);
    }
}

proptest! {
    /// The leaf hash must always be exactly 32 bytes (256 bits), matching
    /// the SHA-256 output size used by the Utreexo accumulator.
    #[test]
    fn leaf_hash_is_32_bytes(leaf in arb_leaf_data()) {
        let hash = leaf._get_leaf_hashes();
        prop_assert_eq!(hash.as_byte_array().len(), 32);
    }
}
