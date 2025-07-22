//#![no_std] // TODO: Make it no_std compatible.

use alloy_rlp::Decodable;

pub mod mpt;

extern crate alloc;

/// Represents Ethereum state.
/// NOTE: In Zeth *similar* fields are part of `StatelessClientData` struct.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Risc0ZkvmTrie {
  pub state_trie: risc0_ethereum_trie::CachedTrie,
  pub storage_tries: alloy_primitives::map::HashMap<
    alloy_primitives::B256,
    risc0_ethereum_trie::CachedTrie,
    alloy_primitives::map::foldhash::fast::RandomState,
  >,
}

impl Risc0ZkvmTrie {
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
      risc0_ethereum_trie::CachedTrie,
      alloy_primitives::map::HashMap<
        alloy_primitives::B256,
        risc0_ethereum_trie::CachedTrie,
        alloy_primitives::map::foldhash::fast::RandomState,
      >,
    ),
    alloc::string::String,
  > {
    // Step 1: Index RLP-encoded nodes by hash.
    // IMPORTANT: Witness state contains both state trie nodes and storage tries nodes!
    let num_nodes = witness.state.len();
    let mut node_map = alloy_primitives::map::B256Map::with_capacity_and_hasher(num_nodes, Default::default());
    for encoded in &witness.state {
      let hash = alloy_primitives::keccak256(encoded);
      node_map.insert(hash, encoded.clone());
    }

    // Step 2: Build state trie.
    let mut state_trie = risc0_ethereum_trie::CachedTrie::from_digest(pre_state_root);
    state_trie.hydrate_from_rlp_map(&node_map).unwrap();

    // Step 3: Find required storage tries.
    // We can use `witness.keys` to discover accounts that were touched.
    let mut storage_tries_detected = alloc::vec![];
    for key_bytes in witness.keys.iter() {
      // Skip non-address keys (U256 slots).
      if key_bytes.len() != 20 {
        continue;
      }

      // Get account from the state trie.
      let hashed_address = alloy_primitives::keccak256(key_bytes);
      let mut account_bytes = match state_trie.get(&hashed_address) {
        Some(account_bytes) => account_bytes,
        None => {
          // This is fine to skip! Account did not exist in the pre-state, but was created later during block execution.
          continue;
        }
      };
      let account = alloy_trie::TrieAccount::decode(&mut account_bytes).expect("Failed to decode account");

      // Store non-empty storage roots.
      let storage_root = account.storage_root;
      if storage_root != alloy_trie::EMPTY_ROOT_HASH {
        storage_tries_detected.push((hashed_address, storage_root));
      }
    }

    // Step 4: Build storage tries.
    let mut storage_tries:
      alloy_primitives::map::HashMap<
        alloy_primitives::B256,
        risc0_ethereum_trie::CachedTrie,
      > = alloy_primitives::map::HashMap::default();
    for (hashed_address, storage_root) in storage_tries_detected {
      let mut storage_trie = risc0_ethereum_trie::CachedTrie::from_digest(storage_root);
      storage_trie.hydrate_from_rlp_map(&node_map).unwrap();
      // node_by_hash.get(&storage_root).unwrap(); // Not sure if this extra check is needed.
      storage_tries
        .insert(hashed_address, storage_trie);
    }

    // Step 3a: Verify that state_trie was built correctly - confirm tree hash with pre state root.
    validate_state_trie(&state_trie, pre_state_root);

    // Step 3b: Verify that each storage trie matches the declared storage_root in the state trie.
    validate_storage_tries(&state_trie, &storage_tries)?;

    Ok((state_trie, storage_tries))
  }

  /// Computes the Zeth state root (over state trie).
  pub fn compute_state_root(&self) -> alloy_primitives::B256 {
    self.state_trie.hash_slow()
  }

  /// Mutates Zeth state based on diffs provided in [`HashedPostState`].
  pub fn update(&mut self, post_state: &reth_trie_common::HashedPostState) {
    // Apply *all* storage-slot updates first and remember new roots.
    let mut new_storage_roots: alloy_primitives::map::HashMap<
      alloc::vec::Vec<u8>,
      alloy_primitives::B256,
    > = alloy_primitives::map::HashMap::with_capacity_and_hasher(post_state.storages.len(), Default::default());
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
          storage_trie.remove(key);
        } else {
          // TODO: Use `insert_rlp` when trie gets wrapped in MPT - https://github.com/risc0/zeth/blob/1ecdfa2325af161b529a4ad4cb2b6ce949679a28/crates/core/src/mpt.rs#L43-L49.
          //storage_trie.insert_rlp(key, *value).unwrap();
          storage_trie.insert(key, alloy_rlp::encode(value));
        }
      }

      // Memorise the freshly-computed root.
      new_storage_roots.insert(hashed_addr.to_vec(), storage_trie.hash_slow()); // TODO: Consider using `hash()`.
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
            .or_else(|| self.storage_tries.get(addr).map(|t| t.hash_slow())) // TODO: Consider using `hash()`.
            .unwrap_or(crate::mpt::EMPTY_ROOT);

          // If both the account and its storage are empty we simply delete.
          if acct.is_empty() && storage_root == crate::mpt::EMPTY_ROOT {
            self.state_trie.remove(addr);
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
          // TODO: Use `insert_rlp` when trie gets wrapped in MPT - https://github.com/risc0/zeth/blob/1ecdfa2325af161b529a4ad4cb2b6ce949679a28/crates/core/src/mpt.rs#L43-L49.
          //self.state_trie.insert_rlp(addr, trie_acct).unwrap();
          self.state_trie.insert(addr, alloy_rlp::encode(trie_acct));
        }

        // Handle account deletion.
        None => {
          self.state_trie.remove(addr);
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

    // TODO: Use get_rlp() when trie gets wrapped in MPT.
    let account_in_trie = match self.state_trie.get(hashed_address) {
      Some(mut account_bytes) => {
      match alloy_trie::TrieAccount::decode(&mut account_bytes) {
        Ok(account) => Some(account),
        Err(_) => panic!("Failed to decode account"),
      }
      },
      None => None
    };

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
      let hashed_slot_bytes = alloy_primitives::keccak256(index.to_be_bytes::<32>());
      let hashed_slot = hashed_slot_bytes.as_slice();

      return Ok(match storage_trie.get(hashed_slot) {
        Some(mut value_bytes) => {
          match alloy_primitives::U256::decode(&mut value_bytes) {
            Ok(value) => value,
            Err(_) => panic!("Failed to decode storage value")
          }
        },
        None => alloy_primitives::U256::ZERO
      });
    }

    // Storage slot value is not present in the trie, validate that the witness is complete.
    // TODO: Implement witness checks like in reth - https://github.com/paradigmxyz/reth/blob/127595e23079de2c494048d0821ea1f1107eb624/crates/stateless/src/trie.rs#L68C9-L87.
    // TODO: Use `get_rlp()` when trie gets wrapped in MPT.
    let account = match self.state_trie.get(hashed_address) {
      Some(mut account_bytes) => {
      match alloy_trie::TrieAccount::decode(&mut account_bytes) {
        Ok(account) => Some(account),
        Err(_) => panic!("Failed to decode account"),
      }
      },
      None => None,
    };
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

impl reth_stateless::StatelessTrie for Risc0ZkvmTrie {
  fn new(
    witness: &reth_stateless::ExecutionWitness,
    pre_state_root: alloy_primitives::B256,
  ) -> Result<
    (Self, alloy_primitives::map::B256Map<revm::state::Bytecode>),
    reth_stateless::validation::StatelessValidationError,
  > {
    let ethereum_state = Self::from_execution_witness(witness, pre_state_root);

    // TODO: This should be like Bytecodes::build_map!!!
    // let bytecodes = Bytecodes::from_execution_witness(witness);
    // let bytecodes = bytecodes.codes;

    // Temporary code below (present in public PR):
    let mut bytecodes: alloy_primitives::map::B256Map<revm::state::Bytecode> =
      alloy_primitives::map::B256Map::default();
    for encoded in &witness.codes {
      let hash = alloy_primitives::keccak256(encoded);
      bytecodes.insert(hash, revm::state::Bytecode::new_raw(encoded.clone()));
    }

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
  state_trie: &risc0_ethereum_trie::CachedTrie,
  pre_state_root: alloy_primitives::FixedBytes<32>,
) {
  // TODO: Consider using `hash()`, but not sure if better.
  if state_trie.hash_slow() != pre_state_root {
    panic!("Computed state root does not match pre_state_root");
  }
}

// Validates that each storage trie matches the declared storage_root in the state trie.
pub fn validate_storage_tries(
  state_trie: &risc0_ethereum_trie::CachedTrie,
  storage_tries: &alloy_primitives::map::HashMap<
    alloy_primitives::B256,
    risc0_ethereum_trie::CachedTrie,
    alloy_primitives::map::foldhash::fast::RandomState,
  >,
) -> Result<(), alloc::string::String> {
  for (hashed_address, storage_trie) in storage_tries.iter() {
    // TODO: Use get_rlp() for getting account, once trie is wrapped in MPT.
    let mut account_bytes = state_trie.get(hashed_address.as_slice())
      .ok_or("Account not found in state trie")?;
    let account = alloy_trie::TrieAccount::decode(&mut account_bytes)
      .map_err(|_| "Failed to decode account from state trie")?;
    let storage_root = account.storage_root;
    let actual_hash = storage_trie.hash_slow(); // TODO: Consider using `hash()`.

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
