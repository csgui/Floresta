use bitcoin::{Transaction, Txid};

/// An abstract interface for interacting with a transaction mempool.
///
/// This trait decouples consumers from the concrete mempool implementation,
/// allowing lower-level crates to depend on this domain-level abstraction
/// rather than on the `floresta-mempool` crate directly.
pub trait MempoolInterface: Send {
    /// Retrieves a transaction from the mempool by its txid.
    fn get_transaction(&self, txid: &Txid) -> Option<&Transaction>;

    /// Lists all transaction IDs currently in the mempool.
    fn list_transactions(&self) -> Vec<Txid>;

    /// Returns transactions that have been in the mempool long enough to be
    /// considered stale and eligible for rebroadcast or eviction.
    fn get_stale(&mut self) -> Vec<Txid>;
}
