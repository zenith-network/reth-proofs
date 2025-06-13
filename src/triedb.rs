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
      println!("Block number: {number}, header: {header:?}");
      block_hashes.insert(number, hash);

      if number > highest_block_number {
        highest_block_number = number;
        highest_state_root = Some(header.state_root);
      }
    }
    let pre_state_root =
      highest_state_root.expect("At least one block header must be present in the witness");

    // Step 1: Decode all RLP-encoded trie nodes and index by hash
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

    let state_trie = crate::mpt::resolve_nodes(&root, &node_map);

    // Step 3: Build storage tries per account efficiently
    let mut storage_tries: alloy_primitives::map::HashMap<
      alloy_primitives::B256,
      crate::mpt::MptNode,
    > = alloy_primitives::map::HashMap::default();

    for key in &witness.keys {
      if key.0.len() != 20 {
        continue; // Not an address preimage
      }

      let address = alloy_primitives::Address::from_slice(&key.0);
      let address_hash = alloy_primitives::keccak256(address);

      if storage_tries.contains_key(&address_hash) {
        continue;
      }

      let account_opt = state_trie
        .get_rlp::<reth_trie::TrieAccount>(address_hash.as_slice())
        .ok()
        .flatten();

      // For each account we care about
      if let Some(account) = account_opt {
        let storage_root = account.storage_root;

        let storage_trie = if storage_root == alloy_primitives::B256::ZERO {
          crate::mpt::MptNode::default()
        } else if let Some(root_node) = node_by_hash.get(&storage_root).cloned() {
          let resolved = crate::mpt::resolve_nodes(&root_node, &node_map);
          if resolved.is_digest() {
            eprintln!("⚠️ Could not fully resolve storage trie for {address}");
            crate::mpt::MptNodeData::Digest(storage_root).into()
          } else {
            resolved
          }
        } else {
          eprintln!("⚠️ Missing storage root node for {address} (hash: {storage_root:?})");
          crate::mpt::MptNodeData::Digest(storage_root).into()
        };

        storage_tries.insert(address_hash, storage_trie);
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

    Ok(Self {
      state_trie,
      storage_tries,
      bytecode_by_hash,
      block_hashes,
    })
  }

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
