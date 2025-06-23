#![no_std]

extern crate alloc;

pub mod mpt;

// It is used in the `BasicBlockExecutor` as "strategy factory", implementing `ConfigureEvm` trait.
// Measured SP1 performance:
// - no precompiles - 501M cycles (deserialization took 64M)
// - enabled `tiny-keccak` feature in `alloy_primitives` + added `tiny-keccak` precompile - 501M cycles (deserialization took 64M)
// - enabled `sha3-keccak` feature in `alloy_primitives` + added `sha3` precompile - 136M cycles (deserialization took 64M)
// - enabled `sha3-keccak` feature in alloy_primitives + no precompiles - 501M cycles (deserialization took 64M)
//
// TIP: In zkVM use more efficient `create_mainnet_evm_config_from` with a bare-minimum chainspec.
pub fn create_mainnet_evm_config() -> reth_ethereum::evm::EthEvmConfig {
  reth_ethereum::evm::EthEvmConfig::mainnet()
}

/// Creates an Ethereum mainnet EVM config from the given chainspec.
/// Measured SP1 performance:
/// - <1K cycles
pub fn create_mainnet_evm_config_from(
  chainspec: alloc::sync::Arc<reth_chainspec::ChainSpec>,
) -> reth_ethereum::evm::EthEvmConfig {
  reth_ethereum::evm::EthEvmConfig::ethereum(chainspec)
}

/// Creates an Ethereum mainnet chainspec.
/// Measured SP1 performance:
/// - 32K cycles (hardfork creation took 21K cycles, could be optimized)
///
/// NOTE: Chainspec is not serializable; genesis is.
///
/// NOTE: We are using chainspec from RSP, which is NOT a 100% Ethereum chainspec.
/// See the "real" mainnet chainspec here:
/// - https://github.com/paradigmxyz/reth/blob/127595e23079de2c494048d0821ea1f1107eb624/crates/chainspec/src/spec.rs#L88-L114
///
/// TODO: See if we can construct more Ethereum-like chainspec, without adding too many cycles.
pub fn create_mainnet_chainspec() -> reth_chainspec::ChainSpec {
  let mainnet = reth_chainspec::ChainSpec {
    chain: reth_chainspec::Chain::mainnet(),
    genesis: Default::default(),
    genesis_header: Default::default(),
    paris_block_and_final_difficulty: Default::default(),
    hardforks: reth_chainspec::EthereumHardfork::mainnet().into(),
    deposit_contract: Default::default(),
    base_fee_params: reth_chainspec::BaseFeeParamsKind::Constant(
      reth_chainspec::BaseFeeParams::ethereum(),
    ),
    prune_delete_limit: 20000,
    blob_params: Default::default(),
  };

  mainnet
}

// One of four main inputs for zkVM execution
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AncestorHeaders {
  // List of ancestors. SHOULD be sorted by block number descending, otherwise validation will fail.
  // NOTE: First item is the 'highest' block from which we get the pre state root hash.
  pub headers: alloc::vec::Vec<alloy_consensus::Header>,
}

// Required for `.number()` calls on sealed blocks.
use alloy_consensus::BlockHeader;

impl AncestorHeaders {
  /// Prepares one of four zkVM inputs - ancestor headers.
  /// Validated data could be retrieved later in zkVM, using `seal_and_validate`.
  pub fn from_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) -> Self {
    // Get headers from the witness, we reverse them to get the desired order.
    // NOTE: Reth provides `ExecutionWitness` with headers sorted by block number ascending:
    // - https://github.com/paradigmxyz/reth/blob/127595e23079de2c494048d0821ea1f1107eb624/crates/rpc/rpc/src/debug.rs#L665-L677
    let mut headers = witness.headers.clone();
    headers.reverse();

    // Decode headers.
    let headers: alloc::vec::Vec<alloy_consensus::Header> = headers
      .into_iter()
      .map(|header_bytes| {
        <alloy_consensus::Header as alloy_rlp::Decodable>::decode(&mut &header_bytes[..])
          .expect("Failed to decode header; witness should be valid")
      })
      .collect();

    Self { headers }
  }

  /// Validates that headers (current block + ancestors) are connected correctly (parent-child relationship),
  /// seals them (to get hash), and returns a map of block numbers to block hashes.
  pub fn seal_and_validate(
    &self,
    current_block: &CurrentBlock,
  ) -> alloy_primitives::map::HashMap<u64, reth_ethereum::evm::revm::primitives::FixedBytes<32>> {
    let mut block_hashes = alloy_primitives::map::HashMap::default();
    let mut sealed_prev: Option<reth_ethereum::primitives::SealedHeader> = None;

    // Chain current block header with ancestor headers.
    let current_header = &current_block.body.header;
    let headers = core::iter::once(current_header).chain(self.headers.iter());

    for header in headers {
      // TODO: Consider avoiding clone.
      let sealed = reth_ethereum::primitives::SealedHeader::seal_slow(header.clone());
      if let Some(prev) = &sealed_prev {
        if sealed.number() != prev.number() - 1 {
          panic!(
            "InvalidHeaderBlockNumber({}, {})",
            prev.number() - 1,
            sealed.number()
          );
        }
        if sealed.hash() != prev.parent_hash() {
          panic!(
            "InvalidHeaderParentHash({}, {})",
            sealed.hash(),
            prev.parent_hash()
          );
        }
        block_hashes.insert(prev.number(), prev.hash());
      }

      sealed_prev = Some(sealed);
    }

    block_hashes
  }
}

/// Second main input for zkVM.
#[serde_with::serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CurrentBlock {
  #[serde_as(
    as = "reth_primitives_traits::serde_bincode_compat::Block<'_, alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>, alloy_consensus::Header>"
  )]
  pub body: alloy_consensus::Block<
    alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>,
    alloy_consensus::Header,
  >,
}

/// Third main input for zkVM.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EthereumState {
  pub state_trie: mpt::MptNode,
  pub storage_tries: alloy_primitives::map::hash_map::HashMap<
    alloy_primitives::FixedBytes<32>,
    mpt::MptNode,
    alloy_primitives::map::foldhash::fast::RandomState,
  >,
}

impl EthereumState {
  /// Prepares one of four zkVM inputs - Ethereum state (state trie, and storage tries).
  /// Deserialized data should be validated in zkVM, using `validate_storage_tries`.
  pub fn from_execution_witness(
    witness: &alloy_rpc_types_debug::ExecutionWitness,
    pre_state_root: alloy_primitives::FixedBytes<32>,
  ) -> Self {
    let (state_trie, storage_tries) = Self::build_validated_tries(witness, pre_state_root).unwrap();
    Self {
      state_trie,
      storage_tries,
    }
  }

  // Builds tries from the witness state.
  // NOTE: This method should be called outside zkVM! In general you construct tries, then validate them inside zkVM.
  pub fn build_validated_tries(
    witness: &alloy_rpc_types_debug::ExecutionWitness,
    pre_state_root: alloy_primitives::FixedBytes<32>,
  ) -> Result<
    (
      mpt::MptNode,
      alloy_primitives::map::hash_map::HashMap<
        alloy_primitives::FixedBytes<32>,
        mpt::MptNode,
        alloy_primitives::map::foldhash::fast::RandomState,
      >,
    ),
    alloc::string::String,
  > {
    // Step 1: Decode all RLP-encoded trie nodes and index by hash
    // IMPORTANT: Witness state contains both *state trie* nodes and *storage tries* nodes!
    let mut node_map: alloy_primitives::map::HashMap<
      crate::mpt::MptNodeReference,
      crate::mpt::MptNode,
    > = alloy_primitives::map::HashMap::default();
    let mut node_by_hash: alloy_primitives::map::HashMap<
      alloy_primitives::B256,
      crate::mpt::MptNode,
    > = alloy_primitives::map::HashMap::default();
    let mut root_node: Option<crate::mpt::MptNode> = None;

    for encoded in &witness.state {
      let node = crate::mpt::MptNode::decode(encoded).expect("Valid MPT node in witness");
      let hash = alloy_primitives::keccak256(encoded);
      if hash == pre_state_root {
        root_node = Some(node.clone());
      }
      node_by_hash.insert(hash, node.clone());
      node_map.insert(node.reference(), node);
    }

    // Step 2: Use root_node or fallback to Digest
    let root = root_node.unwrap_or_else(|| crate::mpt::MptNodeData::Digest(pre_state_root).into());

    // Build state trie.
    let mut storage_tries_detected = alloc::vec![];
    let state_trie = crate::mpt::resolve_state_nodes(
      &root,
      &node_map,
      &mut storage_tries_detected,
      nybbles::Nibbles::default(),
    );

    // Step 3: Build storage tries per account efficiently
    let mut storage_tries: alloy_primitives::map::HashMap<
      alloy_primitives::B256,
      crate::mpt::MptNode,
    > = alloy_primitives::map::HashMap::default();

    for (hashed_address, storage_root) in storage_tries_detected {
      let root_node = match node_by_hash.get(&storage_root).cloned() {
        Some(node) => node,
        None => {
          // An execution witness can include an account leaf (with non-empty storageRoot), but omit
          // its entire storage trie when that account's storage was NOT touched during the block.
          continue;
        }
      };
      let storage_trie = crate::mpt::resolve_nodes(&root_node, &node_map);

      if storage_trie.is_digest() {
        panic!("Could not resolve storage trie for {storage_root}");
      }

      // Insert resolved storage trie.
      storage_tries.insert(hashed_address, storage_trie);
    }

    // Step 3a: Verify that state_trie was built correctly - confirm tree hash with pre state root.
    validate_state_trie(&state_trie, pre_state_root);

    // Step 3b: Verify that each storage trie matches the declared storage_root in the state trie.
    validate_storage_tries(&state_trie, &storage_tries)?;

    Ok((state_trie, storage_tries))
  }
}

// Validate that state_trie was built correctly - confirm tree hash with pre state root.
pub fn validate_state_trie(
  state_trie: &mpt::MptNode,
  pre_state_root: alloy_primitives::FixedBytes<32>,
) {
  if state_trie.hash() != pre_state_root {
    panic!("Computed state root does not match pre_state_root");
  }
}

// Validates that each storage trie matches the declared storage_root in the state trie.
// Measured SP1 performance: ~33M cycles.
pub fn validate_storage_tries(
  state_trie: &mpt::MptNode,
  storage_tries: &alloy_primitives::map::hash_map::HashMap<
    alloy_primitives::FixedBytes<32>,
    mpt::MptNode,
    alloy_primitives::map::foldhash::fast::RandomState,
  >,
) -> Result<(), alloc::string::String> {
  for (hashed_address, storage_trie) in storage_tries.iter() {
    let account = state_trie
      .get_rlp::<alloy_trie::TrieAccount>(hashed_address.as_slice())
      .map_err(|_| "Failed to decode account from state trie")?
      .ok_or("Account not found in state trie")?;

    let storage_root = account.storage_root;
    let actual_hash = storage_trie.hash();

    if storage_root != actual_hash {
      return Err(
        alloc::format!(
          "Mismatched storage root for address hash {:?}: expected {:?}, got {:?}",
          hashed_address,
          storage_root,
          actual_hash
        )
        .into(),
      );
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_create_mainnet_evm_config() {
    let config = create_mainnet_evm_config();
    assert_eq!(
      reth_ethereum::chainspec::EthChainSpec::chain_id(&config.chain_spec()),
      1
    );
  }
}
