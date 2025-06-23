#[derive(Debug)]
pub struct TrieDB {
  pub state_trie: reth_proofs_core::mpt::MptNode,
  pub storage_tries:
    alloy_primitives::map::HashMap<alloy_primitives::B256, reth_proofs_core::mpt::MptNode>,
  pub block_hashes: alloy_primitives::map::HashMap<u64, alloy_primitives::B256>,
  pub bytecode_by_hash:
    alloy_primitives::map::HashMap<alloy_primitives::B256, revm::state::Bytecode>,
}

fn build_tries(
  witness: &alloy_rpc_types_debug::ExecutionWitness,
  pre_state_root: alloy_primitives::FixedBytes<32>,
) -> Result<
  (
    reth_proofs_core::mpt::MptNode,
    alloy_primitives::map::HashMap<alloy_primitives::B256, reth_proofs_core::mpt::MptNode>,
  ),
  Box<dyn std::error::Error>,
> {
  // Step 1: Decode all RLP-encoded trie nodes and index by hash
  // IMPORTANT: Witness state contains both *state trie* nodes and *storage tries* nodes!
  let mut node_map: alloy_primitives::map::HashMap<
    reth_proofs_core::mpt::MptNodeReference,
    reth_proofs_core::mpt::MptNode,
  > = alloy_primitives::map::HashMap::default();
  let mut node_by_hash: alloy_primitives::map::HashMap<
    alloy_primitives::B256,
    reth_proofs_core::mpt::MptNode,
  > = alloy_primitives::map::HashMap::default();
  let mut root_node: Option<reth_proofs_core::mpt::MptNode> = None;

  for encoded in &witness.state {
    let node = reth_proofs_core::mpt::MptNode::decode(encoded).expect("Valid MPT node in witness");
    let hash = alloy_primitives::keccak256(encoded);
    if hash == pre_state_root {
      root_node = Some(node.clone());
    }
    node_by_hash.insert(hash, node.clone());
    node_map.insert(node.reference(), node);
  }

  // Step 2: Use root_node or fallback to Digest
  let root =
    root_node.unwrap_or_else(|| reth_proofs_core::mpt::MptNodeData::Digest(pre_state_root).into());

  // Build state trie.
  let mut storage_tries_detected = vec![];
  let state_trie = reth_proofs_core::mpt::resolve_state_nodes(
    &root,
    &node_map,
    &mut storage_tries_detected,
    reth_trie::Nibbles::default(),
  );

  // Step 3: Build storage tries per account efficiently
  let mut storage_tries: alloy_primitives::map::HashMap<
    alloy_primitives::B256,
    reth_proofs_core::mpt::MptNode,
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
    let storage_trie = reth_proofs_core::mpt::resolve_nodes(&root_node, &node_map);

    if storage_trie.is_digest() {
      panic!("Could not resolve storage trie for {storage_root}");
    }

    // Insert resolved storage trie.
    storage_tries.insert(hashed_address, storage_trie);
  }

  // Step 3b: Verify that each storage trie matches the declared storage_root in the state trie
  for (hashed_address, storage_trie) in storage_tries.iter() {
    let account = state_trie
      .get_rlp::<reth_trie::TrieAccount>(hashed_address.as_slice())
      .map_err(|_| "Failed to decode account from state trie")?
      .ok_or("Account not found in state trie")?;

    let storage_root = account.storage_root;
    let actual_hash = storage_trie.hash();

    if storage_root != actual_hash {
      return Err(
        format!(
          "Mismatched storage root for address hash {:?}: expected {:?}, got {:?}",
          hashed_address, storage_root, actual_hash
        )
        .into(),
      );
    }
  }

  Ok((state_trie, storage_tries))
}

impl TrieDB {
  // Custom integration - written by chatGPT.
  // This is constructing a *valid* TrieDB.
  pub fn from_execution_witness(
    witness: alloy_rpc_types_debug::ExecutionWitness,
    block: &alloy_consensus::Block<reth_ethereum::TransactionSigned>,
  ) -> Result<Self, Box<dyn std::error::Error>> {
    // Step 0: Build block hashes and locate `pre_state_root`.
    let ancestor_headers = reth_proofs_core::AncestorHeaders::from_execution_witness(&witness);
    let block = reth_proofs_core::CurrentBlock {
      body: block.clone(),
    };
    let block_hashes = ancestor_headers.seal_and_validate(&block);
    let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;

    // Step 1-3: Build state trie and storage tries.
    let (state_trie, storage_tries) = build_tries(&witness, pre_state_root)?;

    // Step 4: Build bytecode map
    let mut bytecode_by_hash: alloy_primitives::map::HashMap<
      alloy_primitives::B256,
      revm::state::Bytecode,
    > = alloy_primitives::map::HashMap::default();
    for encoded in &witness.codes {
      let hash = alloy_primitives::keccak256(encoded);
      bytecode_by_hash.insert(hash, revm::state::Bytecode::new_raw(encoded.clone()));
    }

    let trie = Self {
      state_trie,
      storage_tries,
      bytecode_by_hash,
      block_hashes,
    };

    // Extra check to validate that state_trie was built correctly - confirm tree hash with pre state root.
    // Do NOT use this inside zkVM!
    if trie.compute_state_root() != pre_state_root {
      panic!("Error in TrieDB build logic: computed root does not match pre_state_root");
    }

    Ok(trie)
  }

  /// Computes the state root (over state trie).
  pub fn compute_state_root(&self) -> alloy_primitives::B256 {
    self.state_trie.hash()
  }

  /// Mutates state based on diffs provided in [`HashedPostState`].
  pub fn update(&mut self, post_state: &reth_trie::HashedPostState) {
    // Apply *all* storage-slot updates first and remember new roots.
    let mut new_storage_roots: alloy_primitives::map::HashMap<Vec<u8>, alloy_primitives::B256> =
      alloy_primitives::map::HashMap::default(); // TODO: Use `with_capacity(post_state.storages.len())`.
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
            .unwrap_or(reth_proofs_core::mpt::EMPTY_ROOT);

          // If both the account and its storage are empty we simply delete.
          if acct.is_empty() && storage_root == reth_proofs_core::mpt::EMPTY_ROOT {
            self.state_trie.delete(addr).unwrap();
            self.storage_tries.remove(addr); // keep maps in sync
            continue;
          }

          // Encode and insert the account leaf.
          let trie_acct = reth_trie::TrieAccount {
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
}

impl revm::DatabaseRef for TrieDB {
  /// The database error type.
  type Error = reth_ethereum::evm::primitives::execute::ProviderError;

  /// Get basic account information.
  fn basic_ref(
    &self,
    address: alloy_primitives::Address,
  ) -> Result<Option<revm::state::AccountInfo>, Self::Error> {
    let hashed_address = alloy_primitives::keccak256(address);
    let hashed_address = hashed_address.as_slice();

    let account_in_trie = self
      .state_trie
      .get_rlp::<reth_trie::TrieAccount>(hashed_address)
      .unwrap();

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
      .get_rlp::<reth_trie::TrieAccount>(hashed_address)
      .expect("Can get account from MPT");
    match account {
      Some(account) => {
        if account.storage_root != reth_proofs_core::mpt::EMPTY_ROOT {
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

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_triedb_from_execution_witness() {
    let mainnet_reth_nr10 = "http://130.250.187.55:8545";
    let provider = crate::create_provider(mainnet_reth_nr10).unwrap();
    let block_number = crate::get_last_block_number(&provider).await.unwrap();
    let block = crate::fetch_full_block(&provider, block_number)
      .await
      .unwrap()
      .unwrap();
    let witness = crate::fetch_block_witness(&provider, block_number)
      .await
      .unwrap();

    let block = crate::rpc_block_to_consensus_block(block);
    let _trie_db = TrieDB::from_execution_witness(witness, &block)
      .expect("Failed to create TrieDB from execution witness");
  }
}
