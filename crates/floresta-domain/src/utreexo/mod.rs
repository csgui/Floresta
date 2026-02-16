//! Utreexo domain primitives and abstractions.

pub mod compact;
pub mod error;
pub mod leaf;

#[cfg(test)]
mod tests;

pub use compact::CompactLeafData;
pub use compact::ScriptPubKeyKind;
pub use error::LeafErrorKind;
pub use error::UtreexoLeafError;
pub use leaf::LeafData;
pub use leaf::UTREEXO_TAG_V1;
