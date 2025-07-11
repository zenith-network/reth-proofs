#[derive(Debug)]
pub struct TrieDB {
  pub state_trie: crate::mpt::MptNode,
  pub storage_tries: alloy_primitives::map::HashMap<alloy_primitives::B256, crate::mpt::MptNode>,
  pub block_hashes: alloy_primitives::map::HashMap<u64, alloy_primitives::B256>,
  pub bytecode_by_hash:
    alloy_primitives::map::HashMap<alloy_primitives::B256, revm::state::Bytecode>,
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
      state_trie: ethereum_state.state_trie,
      storage_tries: ethereum_state.storage_tries,
      bytecode_by_hash,
      block_hashes,
    };

    Ok(trie)
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
  pub fn account(&self,
    address: alloy_primitives::Address,
  ) -> Result<Option<alloy_trie::TrieAccount>, reth_ethereum::evm::primitives::execute::ProviderError> {
    let hashed_address = alloy_primitives::keccak256(address);
    let hashed_address = hashed_address.as_slice();

    let account_in_trie = self
      .state_trie
      .get_rlp::<alloy_trie::TrieAccount>(hashed_address)
      .unwrap();

    Ok(account_in_trie)
  }

  // NOTE: It provides 1-to-1 mapping with `StatelessTrie::storage`.
  pub fn storage(&self,
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

impl revm::DatabaseRef for TrieDB {
  /// The database error type.
  type Error = reth_ethereum::evm::primitives::execute::ProviderError;

  /// Get basic account information.
  fn basic_ref(
    &self,
    address: alloy_primitives::Address,
  ) -> Result<Option<revm::state::AccountInfo>, Self::Error> {
    let account_in_trie = self.account(address)?;
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
    self.storage(address, index)
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
