#![no_std]

extern crate alloc;

pub mod input;
pub mod mpt;
pub mod triedb;

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
        block_hashes.insert(sealed.number(), sealed.hash());
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

impl CurrentBlock {
  // TODO: By taking `self` we could avoid cloning body.
  pub fn recover_senders(
    &self,
  ) -> reth_primitives_traits::RecoveredBlock<
    alloy_consensus::Block<alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>>,
  > {
    reth_primitives_traits::RecoveredBlock::try_recover(self.body.clone()).unwrap()
  }
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

  /// Computes the state root (over state trie).
  pub fn compute_state_root(&self) -> alloy_primitives::B256 {
    self.state_trie.hash()
  }

  /// Mutates state based on diffs provided in [`HashedPostState`].
  pub fn update(&mut self, post_state: &reth_trie_common::HashedPostState) {
    // Apply *all* storage-slot updates first and remember new roots.
    let mut new_storage_roots: alloy_primitives::map::HashMap<
      alloc::vec::Vec<u8>,
      alloy_primitives::B256,
    > = alloy_primitives::map::HashMap::default(); // TODO: Use `with_capacity(post_state.storages.len())`.
    for (hashed_addr, storage) in post_state.storages.iter() {
      // Take existing storage trie or create an empty one.
      let storage_trie = self.storage_tries.entry(*hashed_addr).or_default();

      // Wipe the trie if requested.
      if storage.wiped {
        storage_trie.clear();
      }

      // Apply slot-level changes.
      for (slot, value) in storage.storage.iter() {
        let key = slot.as_slice();
        if value.is_zero() {
          storage_trie.delete(key).unwrap();
        } else {
          storage_trie.insert_rlp(key, *value).unwrap();
        }
      }

      // Memorise the freshly-computed root.
      new_storage_roots.insert(hashed_addr.to_vec(), storage_trie.hash());
    }

    // Walk the accounts, using the roots computed above.
    for (hashed_addr, maybe_acct) in post_state.accounts.iter() {
      let addr = hashed_addr.as_slice();

      match maybe_acct {
        // Handle account update / creation.
        Some(acct) => {
          // Which storage root should we encode?
          let storage_root = new_storage_roots
            .get(addr)
            .copied() // root from step 1
            .or_else(|| self.storage_tries.get(addr).map(|t| t.hash()))
            .unwrap_or(crate::mpt::EMPTY_ROOT);

          // If both the account and its storage are empty we simply delete.
          if acct.is_empty() && storage_root == crate::mpt::EMPTY_ROOT {
            self.state_trie.delete(addr).unwrap();
            self.storage_tries.remove(addr); // keep maps in sync
            continue;
          }

          // Encode and insert the account leaf.
          let trie_acct = alloy_trie::TrieAccount {
            nonce: acct.nonce,
            balance: acct.balance,
            storage_root,
            code_hash: acct.get_bytecode_hash(),
          };
          self.state_trie.insert_rlp(addr, trie_acct).unwrap();
        }

        // Handle account deletion.
        None => {
          self.state_trie.delete(addr).unwrap();
          self.storage_tries.remove(addr); // NOTE: Could be skipped in zkVM.
        }
      }
    }
  }

  // NOTE: It provides 1-to-1 mapping with `StatelessTrie::account`.
  pub fn account(
    &self,
    address: alloy_primitives::Address,
  ) -> Result<Option<alloy_trie::TrieAccount>, reth_ethereum::evm::primitives::execute::ProviderError>
  {
    let hashed_address = alloy_primitives::keccak256(address);
    let hashed_address = hashed_address.as_slice();

    let account_in_trie = self
      .state_trie
      .get_rlp::<alloy_trie::TrieAccount>(hashed_address)
      .unwrap();

    Ok(account_in_trie)
  }

  // NOTE: It provides 1-to-1 mapping with `StatelessTrie::storage`.
  pub fn storage(
    &self,
    address: alloy_primitives::Address,
    index: alloy_primitives::U256,
  ) -> Result<alloy_primitives::U256, reth_ethereum::evm::primitives::execute::ProviderError> {
    let hashed_address = alloy_primitives::keccak256(address);
    let hashed_address = hashed_address.as_slice();

    // Usual case, where given storage slot is present.
    if let Some(storage_trie) = self.storage_tries.get(hashed_address) {
      return Ok(
        storage_trie
          .get_rlp::<alloy_primitives::U256>(
            alloy_primitives::keccak256(index.to_be_bytes::<32>()).as_slice(),
          )
          .expect("Can get storage from MPT")
          .unwrap_or_default(),
      );
    }

    // Storage slot value is not present in the trie, validate that the witness is complete.
    // TODO: Implement witness checks like in reth - https://github.com/paradigmxyz/reth/blob/127595e23079de2c494048d0821ea1f1107eb624/crates/stateless/src/trie.rs#L68C9-L87.
    let account = self
      .state_trie
      .get_rlp::<alloy_trie::TrieAccount>(hashed_address)
      .expect("Can get account from MPT");
    match account {
      Some(account) => {
        if account.storage_root != crate::mpt::EMPTY_ROOT {
          todo!("Validate that storage witness is valid");
        }
      }
      None => {
        todo!("Validate that account witness is valid");
      }
    }

    // Account doesn't exist or has empty storage root.
    Ok(alloy_primitives::U256::ZERO)
  }
}

impl reth_stateless::StatelessTrie for EthereumState {
  fn new(
    witness: &reth_stateless::ExecutionWitness,
    pre_state_root: alloy_primitives::B256,
  ) -> Result<
    (Self, alloy_primitives::map::B256Map<revm::state::Bytecode>),
    reth_stateless::validation::StatelessValidationError,
  > {
    let ethereum_state = Self::from_execution_witness(witness, pre_state_root);

    let bytecodes = Bytecodes::from_execution_witness(witness);
    let bytecodes = bytecodes.codes;

    Ok((ethereum_state, bytecodes))
  }

  fn account(
    &self,
    address: alloy_primitives::Address,
  ) -> Result<Option<alloy_trie::TrieAccount>, reth_ethereum::evm::primitives::execute::ProviderError>
  {
    self.account(address)
  }

  fn storage(
    &self,
    address: alloy_primitives::Address,
    index: alloy_primitives::U256,
  ) -> Result<alloy_primitives::U256, reth_ethereum::evm::primitives::execute::ProviderError> {
    self.storage(address, index)
  }

  fn calculate_state_root(
    &mut self,
    state: reth_trie_common::HashedPostState,
  ) -> Result<alloy_primitives::B256, reth_stateless::validation::StatelessValidationError> {
    self.update(&state);
    Ok(self.compute_state_root())
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

// Fourth main input for zkVM.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bytecodes {
  pub codes: alloy_primitives::map::B256Map<revm::state::Bytecode>,
}

impl Bytecodes {
  /// Prepares one of four zkVM inputs - bytecodes.
  pub fn from_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) -> Self {
    let codes = Self::build_map(&witness.codes);
    Self { codes }
  }

  pub fn build_map(
    codes_list: &alloc::vec::Vec<alloy_primitives::Bytes>
  ) -> alloy_primitives::map::B256Map<revm::state::Bytecode> {
    let mut bytecode_by_hash: alloy_primitives::map::B256Map<
      revm::state::Bytecode,
    > = alloy_primitives::map::B256Map::default();
    for encoded in codes_list {
      let bytecode = revm::state::Bytecode::new_raw(encoded.clone());
      let code_hash = bytecode.hash_slow();
      bytecode_by_hash.insert(code_hash, bytecode);
    }
    bytecode_by_hash
  }

  /// Used inside zkVM to validate bytecodes map (correctness of keys).
  pub fn validate(&self) {
    for (hash, bytecode) in &self.codes {
      let computed = bytecode.hash_slow();
      if &computed != hash {
        panic!("Invalid bytecode hash: expected {:?}, got {:?}", hash, computed);
      }
    }
  }
}

pub fn validate_block_post_execution(
  recovered_block: &reth_primitives_traits::RecoveredBlock<
    alloy_consensus::Block<alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>>,
  >,
  chain_spec: &reth_chainspec::ChainSpec,
  execution_output: &reth_execution_types::BlockExecutionOutput<reth_ethereum_primitives::Receipt>,
) {
  if let Err(e) = reth_ethereum_consensus::validate_block_post_execution(
    recovered_block,
    chain_spec,
    &execution_output.result.receipts,
    &execution_output.result.requests,
  ) {
    panic!(
      "Error::Validation(\"Block validation failed after execution: {}\")",
      e
    );
  }
}

pub fn get_hashed_post_state(
  output: &reth_execution_types::BlockExecutionOutput<reth_ethereum_primitives::Receipt>,
) -> reth_trie_common::HashedPostState {
  // Convert the post state from the output to a hashed post state.
  // This is used for further validation or execution.
  reth_trie_common::HashedPostState::from_bundle_state::<reth_trie_common::KeccakKeyHasher>(
    &output.state.state,
  )
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_create_mainnet_evm_config() {
    let config = super::create_mainnet_evm_config();
    assert_eq!(
      reth_ethereum::chainspec::EthChainSpec::chain_id(&config.chain_spec()),
      1
    );
  }
}
