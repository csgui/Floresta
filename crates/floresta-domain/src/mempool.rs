use bitcoin::Transaction;
use bitcoin::Txid;

pub trait MempoolInterface: Send {
    /// Retrieves a transaction from the mempool by its txid.
    fn get_transaction(&self, txid: &Txid) -> Option<&Transaction>;
    // TODO this is an alias to get_transaction. Should we keep both?
    fn get_from_mempool(&self, txid: &Txid) -> Option<&Transaction>;
    /// Lists all transaction IDs currently in the mempool.
    fn list_transactions(&self) -> Vec<Txid>;
}
