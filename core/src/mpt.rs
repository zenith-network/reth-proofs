/// Represents the ways in which one node can reference another node inside the sparse
/// Merkle Patricia Trie (MPT).
///
/// Nodes in the MPT can reference other nodes either directly through their byte
/// representation or indirectly through a hash of their encoding. This enum provides a
/// clear and type-safe way to represent these references.
#[derive(
  Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum MptNodeReference {
  /// Represents a direct reference to another node using its byte encoding. Typically
  /// used for short encodings that are less than 32 bytes in length.
  Bytes(alloc::vec::Vec<u8>),
  /// Represents an indirect reference to another node using the Keccak hash of its long
  /// encoding. Used for encodings that are not less than 32 bytes in length.
  Digest(alloy_primitives::B256),
}
