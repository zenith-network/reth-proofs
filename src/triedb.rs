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
  pub fn from_execution_witness(
    witness: alloy_rpc_types_debug::ExecutionWitness,
  ) -> Result<Self, Box<dyn std::error::Error>> {
    // Step 0: Build block hashes and locate `pre_state_root`.
    let mut block_hashes = alloy_primitives::map::HashMap::default();
    let mut highest_block_number = 0;
    let mut highest_state_root = None;
    for header_bytes in &witness.headers {
      let header =
        <alloy_consensus::Header as alloy_rlp::Decodable>::decode(&mut &header_bytes[..])?;
      let number = header.number;
      let hash = alloy_primitives::keccak256(alloy_rlp::encode(&header));
      block_hashes.insert(number, hash);

      if number > highest_block_number {
        highest_block_number = number;
        highest_state_root = Some(header.state_root);
      }
    }
    let pre_state_root =
      highest_state_root.expect("At least one block header must be present in the witness");

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
      let node = crate::mpt::MptNode::decode(encoded)?;
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
    let mut storage_tries_detected = vec![];
    let state_trie = crate::mpt::resolve_state_nodes(
      &root,
      &node_map,
      &mut storage_tries_detected,
      reth_trie::Nibbles::default(),
    );

    // Step 3: Build storage tries per account efficiently
    let mut storage_tries: alloy_primitives::map::HashMap<
      alloy_primitives::B256,
      crate::mpt::MptNode,
    > = alloy_primitives::map::HashMap::default();
    for (hashed_address, storage_root) in storage_tries_detected {
      let root_node = node_by_hash.get(&storage_root).cloned().unwrap();
      let storage_trie = crate::mpt::resolve_nodes(&root_node, &node_map);

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

  // NOTE: This function can be probably removed, as RSP uses this only in the host
  // for constructing before/after storage proofs.
  pub fn get_state_requests(
    witness: &alloy_rpc_types_debug::ExecutionWitness,
  ) -> alloy_primitives::map::HashMap<alloy_primitives::Address, Vec<alloy_primitives::U256>> {
    let mut requests: alloy_primitives::map::HashMap<
      alloy_primitives::Address,
      std::collections::HashSet<alloy_primitives::U256>,
    > = alloy_primitives::map::HashMap::default();

    for key in &witness.keys {
      match key.0.len() {
        20 => {
          // alloy_primitives::Address only (no storage slot)
          let address = alloy_primitives::Address::from_slice(&key.0);
          requests.entry(address).or_default();
        }
        52 => {
          // alloy_primitives::Address + slot
          let address = alloy_primitives::Address::from_slice(&key.0[..20]);
          let slot = alloy_primitives::U256::from_be_bytes::<32>(key.0[20..].try_into().unwrap());
          requests.entry(address).or_default().insert(slot);
        }
        _ => {
          // Ignore anything else
        }
      }
    }

    // Convert to Vec<U256>
    requests
      .into_iter()
      .map(|(addr, slots)| (addr, slots.into_iter().collect()))
      .collect()
  }

  /// Computes the state root (over state trie).
  pub fn compute_state_root(&self) -> alloy_primitives::B256 {
    self.state_trie.hash()
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

    let storage_trie = self
      .storage_tries
      .get(hashed_address)
      .expect("A storage trie must be provided for each account");

    Ok(
      storage_trie
        .get_rlp::<alloy_primitives::U256>(
          alloy_primitives::keccak256(index.to_be_bytes::<32>()).as_slice(),
        )
        .expect("Can get from MPT")
        .unwrap_or_default(),
    )
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
    let witness = crate::fetch_block_witness(&provider, block_number)
      .await
      .unwrap();

    let _trie_db = TrieDB::from_execution_witness(witness)
      .expect("Failed to create TrieDB from execution witness");
  }
}
