use alloy_primitives::{b256, B256};

/// Root hash of an empty Merkle Patricia trie, i.e. `keccak256(RLP(""))`.
pub const EMPTY_ROOT: B256 =
  b256!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421");
