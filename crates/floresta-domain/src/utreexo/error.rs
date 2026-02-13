use core::fmt;
use std::fmt::Display;
use std::fmt::Formatter;

use bitcoin::script;
use bitcoin::Txid;
use floresta_common::impl_error_from;

use crate::utreexo::CompactLeafData;

#[derive(Debug)]
/// Errors that may occur while reconstructing a leaf's scriptPubKey.
pub enum LeafErrorKind {
    /// The witness or scriptsig was empty, so nothing could be inspected.
    EmptyStack,

    /// The scriptsig data could not be parsed into `Instruction`s.
    InvalidInstruction(script::Error),

    /// The last instruction in the scriptsig was not an `OP_PUSHBYTES`.
    NotPushBytes,
}

impl_error_from!(LeafErrorKind, script::Error, InvalidInstruction);

/// Error while reconstructing a leaf's scriptPubKey, returned by `process_proof`.
///
/// This error is triggered if the input lacks the hashed data required by the
/// [ScriptPubKeyKind] (i.e., the public key for P2PKH, the redeem script for P2SH, or the
/// witness public key and witness script for P2WPKH/P2WSH).
#[derive(Debug)]
pub struct UtreexoLeafError {
    pub leaf: CompactLeafData,
    pub txid: Txid,
    pub vin: usize,
    pub kind: LeafErrorKind,
}

impl Display for UtreexoLeafError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to reconstruct leaf {:?} for TxIn {}:{}: {:?}",
            self.leaf, self.txid, self.vin, self.kind
        )
    }
}
