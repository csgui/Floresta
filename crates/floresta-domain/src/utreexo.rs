use bitcoin::BlockHash;
use bitcoin::OutPoint;
use bitcoin::Transaction;
use bitcoin::TxOut;
use rustreexo::accumulator::node_hash::BitcoinNodeHash;
use rustreexo::accumulator::proof::Proof;

use crate::mempool::MempoolInterface;
#[derive(Debug, PartialEq)]
pub struct LeafData {
    /// A commitment to the block creating this utxo
    pub block_hash: BlockHash,
    /// The utxo's outpoint
    pub prevout: OutPoint,
    pub header_code: u32,
    /// The actual utxo
    pub utxo: TxOut,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct CompactLeafData {
    /// Header code tells the height of creating for this UTXO and whether it's a coinbase
    pub header_code: u32,
    /// The amount locked in this UTXO
    pub amount: u64,
    /// The type of the locking script for this UTXO
    pub spk_ty: ScriptPubKeyKind,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ScriptPubKeyKind {
    /// An non-specified type, in this case the script is just copied over
    Other(Box<[u8]>),
    /// p2pkh
    PubKeyHash,
    /// p2wsh
    WitnessV0PubKeyHash,
    /// p2sh
    ScriptHash,
    /// p2wsh
    WitnessV0ScriptHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptToMempoolError {
    /// The proof provided is invalid.
    InvalidProof,
    /// The transaction is trying to spend an output that we don't have.
    InvalidPrevout,
    /// Memory usage is too high.
    MemoryUsageTooHigh,
    /// We couldn't find a prevout in the mempool.
    ///
    /// This error only happens when we try to add a transaction without a proof, and we don't have
    /// the prevouts in the mempool.
    PrevoutNotFound,
    /// The transaction is conflicting with another transaction in the mempool.
    ConflictingTransaction,
    /// An error happened while trying to get a proof from the accumulator.
    Rustreexo(String),
    /// The transaction has duplicate inputs.
    DuplicateInput,
    BlockNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MempoolProof {
    /// The actual utreexo proof
    pub proof: Proof,
    /// The target hashes that we are trying to prove.
    pub target_hashes: Vec<BitcoinNodeHash>,
    /// The leaf data for the targets we are proving
    pub leaves: Vec<CompactLeafData>,
}

pub trait BlockHashOracle {
    fn get_block_hash(&self, height: u32) -> Option<BlockHash>;
}

pub trait UtreexoMempool: MempoolInterface {
    fn try_prove(
        &self,
        tx: &Transaction,
        chain: &dyn BlockHashOracle,
    ) -> Result<MempoolProof, AcceptToMempoolError>;

    fn accept_to_mempool(
        &mut self,
        transaction: Transaction,
        proof: Proof,
        prevouts: &[(OutPoint, CompactLeafData)],
        del_hashes: &[BitcoinNodeHash],
        remembers: &[u64],
    ) -> Result<(), AcceptToMempoolError>;
}
