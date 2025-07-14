#[derive(Debug)]
pub struct TrieDB {
  pub state: crate::EthereumState,
  pub block_hashes: alloy_primitives::map::HashMap<u64, alloy_primitives::B256>,
  pub bytecode_by_hash: alloy_primitives::map::B256Map<revm::state::Bytecode>,
}

impl TrieDB {
  // Custom integration - written by chatGPT.
  // This is constructing a *valid* TrieDB.
  pub fn from_execution_witness(
    witness: &alloy_rpc_types_debug::ExecutionWitness,
    block: &alloy_consensus::Block<reth_ethereum::TransactionSigned>,
  ) -> Result<Self, alloc::string::String> {
    // Step 0: Build block hashes and locate `pre_state_root`.
    let ancestor_headers = crate::AncestorHeaders::from_execution_witness(witness);
    let block = crate::CurrentBlock {
      body: block.clone(),
    };
    let block_hashes = ancestor_headers.seal_and_validate(&block);
    let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;

    // Step 1-3: Build state trie and storage tries.
    // NOTE: Tries are validated during construction.
    let ethereum_state = crate::EthereumState::from_execution_witness(witness, pre_state_root);

    // Step 4: Build bytecode map.
    let bytecodes = crate::Bytecodes::from_execution_witness(witness);
    let bytecode_by_hash = bytecodes.build_map();

    let trie = Self {
      state: ethereum_state,
      bytecode_by_hash,
      block_hashes,
    };

    Ok(trie)
  }
}

impl revm::DatabaseRef for TrieDB {
  /// The database error type.
  type Error = reth_ethereum::evm::primitives::execute::ProviderError;

  /// Get basic account information.
  fn basic_ref(
    &self,
    address: alloy_primitives::Address,
  ) -> Result<Option<revm::state::AccountInfo>, Self::Error> {
    let account_in_trie = self.state.account(address)?;
    let account = account_in_trie.map(|account_in_trie| revm::state::AccountInfo {
      balance: account_in_trie.balance,
      nonce: account_in_trie.nonce,
      code_hash: account_in_trie.code_hash,
      code: None,
    });

    Ok(account)
  }

  /// Get account code by its hash.
  fn code_by_hash_ref(
    &self,
    hash: alloy_primitives::B256,
  ) -> Result<revm::state::Bytecode, Self::Error> {
    Ok(
      self
        .bytecode_by_hash
        .get(&hash)
        .map(|code| (*code).clone())
        .unwrap(),
    )
  }

  /// Get storage value of address at index.
  fn storage_ref(
    &self,
    address: alloy_primitives::Address,
    index: alloy_primitives::U256,
  ) -> Result<alloy_primitives::U256, Self::Error> {
    self.state.storage(address, index)
  }

  /// Get block hash by block number.
  fn block_hash_ref(&self, number: u64) -> Result<alloy_primitives::B256, Self::Error> {
    Ok(
      *self
        .block_hashes
        .get(&number)
        .expect("A block hash must be provided for each block number"),
    )
  }
}

pub fn wrap_into_database(trie_db: &TrieDB) -> revm::database::WrapDatabaseRef<&TrieDB> {
  let db = revm::database::WrapDatabaseRef(trie_db);
  db
}
