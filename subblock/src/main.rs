use alloy_consensus::BlockHeader;
use reth_evm::ConfigureEvm;
use reth_evm::execute::Executor;
use reth_stateless::StatelessTrie;

mod tracking_trie;
mod witness_db;

extern crate alloc;

#[tokio::main]
async fn main() {
  let block_number = 23449600_u64; // The latest block number that is stored in assets.

  // Part 1.
  // Load the block and witness from files.
  let mut block = reth_proofs::load_block_from_file(block_number)
    .await
    .unwrap();
  let witness = reth_proofs::load_block_witness_from_file(block_number)
    .await
    .unwrap();
  println!("Loaded witness for block {}", block_number);
  println!("Number of state nodes in witness: {}", witness.state.len());
  println!("Number of codes in witness: {}", witness.codes.len());
  println!("Number of keys in witness: {}", witness.keys.len());
  println!("Number of headers in witness: {}", witness.headers.len());

  // Part 1.5: Calculate potential subblock ranges (informational)
  println!("\n=== Calculating Potential Subblock Ranges ===");
  let original_tx_count = block.transactions.len();
  println!("Original block has {} transactions", original_tx_count);

  // Execute full block to get per-transaction gas usage
  println!("Executing full block to measure gas per transaction...");
  let full_block_clone = block.clone();
  let full_block_recovered = reth_proofs::recover_rpc_block(full_block_clone).unwrap();

  // Setup ancestor headers for full block execution
  let mut ancestor_headers_temp: Vec<_> = witness
    .headers
    .iter()
    .map(|bytes| {
      let hash = alloy_primitives::keccak256(bytes);
      alloy_rlp::decode_exact::<alloy_consensus::Header>(bytes)
        .map(|h| reth_primitives_traits::SealedHeader::new(h, hash))
    })
    .collect::<Result<_, _>>()
    .unwrap();
  ancestor_headers_temp.sort_by_key(|header| header.number());
  let ancestor_hashes_temp =
    compute_ancestor_hashes(&full_block_recovered, &ancestor_headers_temp).unwrap();
  let parent_temp = ancestor_headers_temp.last().unwrap();

  // Build trie and execute full block
  let (trie_temp, bytecode_temp) =
    reth_trie_risc0_zkvm::Risc0ZkvmTrie::new(&witness, parent_temp.state_root).unwrap();
  let db_temp = witness_db::WitnessDatabase::new(&trie_temp, bytecode_temp, ancestor_hashes_temp);
  let evm_config_temp = reth_proofs_core::create_mainnet_evm_config();
  let executor_temp = evm_config_temp.executor(db_temp);
  let full_result = executor_temp.execute(&full_block_recovered).unwrap();

  // Calculate subblock ranges with 1M gas limit
  const SUBBLOCK_GAS_LIMIT: u64 = 1_000_000;
  let mut subblock_ranges = Vec::new();
  let mut current_range_start = 0;
  let mut current_range_gas = 0u64;

  for (tx_idx, receipt) in full_result.receipts.iter().enumerate() {
    let tx_gas = receipt.cumulative_gas_used
      - if tx_idx > 0 {
        full_result.receipts[tx_idx - 1].cumulative_gas_used
      } else {
        0
      };

    // If this tx alone exceeds the limit, it gets its own subblock
    if tx_gas >= SUBBLOCK_GAS_LIMIT {
      // Finalize previous range if exists
      if current_range_start < tx_idx {
        subblock_ranges.push((current_range_start, tx_idx - 1, current_range_gas));
      }
      // This large tx gets its own subblock
      subblock_ranges.push((tx_idx, tx_idx, tx_gas));
      current_range_start = tx_idx + 1;
      current_range_gas = 0;
      continue;
    }

    // Check if adding this tx would exceed the limit
    if current_range_gas + tx_gas > SUBBLOCK_GAS_LIMIT && tx_idx > current_range_start {
      // Finalize current range without this tx
      subblock_ranges.push((current_range_start, tx_idx - 1, current_range_gas));
      current_range_start = tx_idx;
      current_range_gas = tx_gas;
    } else {
      // Add this tx to current range
      current_range_gas += tx_gas;
    }
  }

  // Finalize last range
  if current_range_start < original_tx_count {
    subblock_ranges.push((
      current_range_start,
      original_tx_count - 1,
      current_range_gas,
    ));
  }

  // Print subblock ranges
  println!(
    "\nPotential subblocks with {}M gas limit:",
    SUBBLOCK_GAS_LIMIT / 1_000_000
  );
  println!("Total subblocks: {}", subblock_ranges.len());
  for (i, (start, end, gas)) in subblock_ranges.iter().enumerate() {
    let tx_count = end - start + 1;
    println!(
      "  Subblock {}: tx{:02}-tx{:02} ({} txs, {} gas)",
      i, start, end, tx_count, gas
    );
  }
  println!();

  // Continue with existing code: Filter to first 3 transactions for subblock
  println!("=== Executing First Subblock (tx00-tx02) ===");
  let first_n_txs = 3;
  reth_proofs::filter_block_txs_in_place(&mut block, |i, _tx| (0..first_n_txs).contains(&i));
  let filtered_count = block.transactions.len();
  println!("Subblock will execute {} transactions", filtered_count);

  let current_block = reth_proofs::recover_rpc_block(block).unwrap();

  // Part 2.
  // WitnessDatabase construction with reth-stateless code.
  let mut ancestor_headers: Vec<_> = witness
    .headers
    .iter()
    .map(|bytes| {
      let hash = alloy_primitives::keccak256(bytes);
      alloy_rlp::decode_exact::<alloy_consensus::Header>(bytes)
        .map(|h| reth_primitives_traits::SealedHeader::new(h, hash))
    })
    .collect::<Result<_, _>>()
    .expect("StatelessValidationError::HeaderDeserializationFailed");
  // Sort the headers by their block number to ensure that they are in
  // ascending order.
  ancestor_headers.sort_by_key(|header| header.number());

  // Check that the ancestor headers form a contiguous chain and are not just random headers.
  let ancestor_hashes = compute_ancestor_hashes(&current_block, &ancestor_headers).unwrap();

  // There should be at least one ancestor header.
  // The edge case here would be the genesis block, but we do not create proofs for the genesis
  // block.
  let parent = match ancestor_headers.last() {
    Some(prev_header) => prev_header,
    None => panic!("StatelessValidationError::MissingAncestorHeader"),
  };

  // TODO: Validate block against pre-execution consensus rules
  // validate_block_consensus(chain_spec.clone(), &current_block, parent)?;

  // Part 3.
  // Build a tracking trie that wraps Risc0ZkvmTrie and records accessed addresses
  let (tracking_trie, bytecode) = {
    // Build the inner trie
    let (trie, bytecode) =
      reth_trie_risc0_zkvm::Risc0ZkvmTrie::new(&witness, parent.state_root).unwrap();

    // Create tracker
    let tracker = tracking_trie::AccessTracker::new();

    // Wrap it with tracking
    let wrapped = tracking_trie::TrackingTrie::new(trie, tracker);

    (wrapped, bytecode)
  };

  // Create an in-memory database that will use the reads to validate the block.
  let db =
    witness_db::WitnessDatabase::new(&tracking_trie, bytecode.clone(), ancestor_hashes.clone());

  // Part 4.
  // Execute block - the tracker will record all accessed addresses
  println!("\n=== Executing subblock ===");
  let evm_config = reth_proofs_core::create_mainnet_evm_config();
  let executor = evm_config.executor(db);
  let result = executor.execute(&current_block).unwrap();
  println!("✓ Subblock executed successfully!");
  println!("Gas used: {}", result.gas_used);

  // Part 5.
  // Print what was accessed during execution
  let tracker = tracking_trie.tracker();
  let accessed_accounts = tracker.get_accessed_accounts();
  let accessed_storage = tracker.get_accessed_storage();

  println!("\n=== Accessed State ===");
  println!("Accessed accounts: {}", accessed_accounts.len());
  println!("Accessed storage slots: {}", accessed_storage.len());

  println!("\nAll accessed accounts:");
  for addr in accessed_accounts.iter() {
    //}.take(5) {
    println!("  {}", addr);
  }

  println!("\nAll accessed storage slots:");
  for (addr, slot) in accessed_storage.iter() {
    //}.take(5) {
    println!("  {} @ slot {}", addr, slot);
  }

  // Part 6.
  // Clone the trie and re-execute to verify cloning works
  println!("\n=== Testing Trie Cloning ===");
  println!("Cloning the trie...");

  let cloned_trie = tracking_trie.into_inner().clone();
  println!("✓ Trie cloned successfully");

  // Print trie statistics before pruning
  println!("\nTrie statistics (before pruning):");
  println!("  State trie nodes: {}", cloned_trie.state_trie.size());
  println!("  Storage tries: {}", cloned_trie.storage_tries.len());

  println!("✓ Trie cloned successfully - ready for pruning!");

  // Part 7.
  // TODO: Prune the trie based on tracked accesses
  // - Keep only accounts in accessed_accounts
  // - Keep only storage tries for those accounts
  // - For each storage trie, keep only slots in accessed_storage

  // Calculate how many accounts have storage
  let accounts_with_storage: std::collections::HashSet<_> =
    accessed_storage.iter().map(|(addr, _slot)| addr).collect();

  // Part 8. Prune storage tries
  println!("\n=== Pruning Storage Tries ===");
  println!("Before: {} storage tries", cloned_trie.storage_tries.len());

  // Step 1: Compute hashes of accounts with storage accessed (more aggressive)
  let accessed_account_hashes: std::collections::HashSet<alloy_primitives::B256> =
    accounts_with_storage
      .iter()
      .map(|addr| alloy_primitives::keccak256(addr.as_slice()))
      .collect();

  println!(
    "Computed {} hashed addresses from accounts with storage accessed",
    accessed_account_hashes.len()
  );

  // Step 2: Remove storage tries not in accessed accounts
  let mut pruned_trie = cloned_trie.clone();
  pruned_trie
    .storage_tries
    .retain(|hashed_addr, _storage_trie| accessed_account_hashes.contains(hashed_addr));

  println!("After: {} storage tries", pruned_trie.storage_tries.len());
  println!(
    "Removed: {} storage tries",
    cloned_trie.storage_tries.len() - pruned_trie.storage_tries.len()
  );

  // Compute total storage trie size (sum of all storage trie nodes)
  let total_storage_size_before: usize = pruned_trie.storage_tries.values().map(|t| t.size()).sum();
  println!("Total storage trie nodes: {}", total_storage_size_before);

  // Part 8b. Prune storage slots within each storage trie
  println!("\n=== Pruning Storage Slots ===");
  println!("Accessed storage slots: {}", accessed_storage.len());

  // Group accessed slots by address
  let mut slots_by_address: std::collections::HashMap<
    alloy_primitives::Address,
    std::collections::HashSet<alloy_primitives::B256>,
  > = std::collections::HashMap::new();
  for (addr, slot) in &accessed_storage {
    let hashed_slot = alloy_primitives::keccak256(slot.to_be_bytes::<32>());
    slots_by_address
      .entry(*addr)
      .or_default()
      .insert(hashed_slot);
  }

  println!(
    "Grouped into {} addresses with storage access",
    slots_by_address.len()
  );

  // Prune storage slots within each storage trie
  for (hashed_addr, storage_trie) in pruned_trie.storage_tries.iter_mut() {
    for addr in &accessed_accounts {
      if alloy_primitives::keccak256(addr.as_slice()) == *hashed_addr {
        if let Some(target_slots) = slots_by_address.get(addr) {
          let size_before = storage_trie.size();
          storage_trie.prune_to_keys(target_slots);
          let size_after = storage_trie.size();

          if size_before != size_after {
            println!(
              "  Pruned {:?}: {} → {} nodes",
              addr, size_before, size_after
            );
          }
        }
        break;
      }
    }
  }

  let total_storage_size_after: usize = pruned_trie.storage_tries.values().map(|t| t.size()).sum();
  println!(
    "Total storage trie nodes after slot pruning: {}",
    total_storage_size_after
  );

  println!("\nTrie statistics (storage tries pruned, slots NOT pruned):");
  println!(
    "  State trie nodes: {} (unchanged)",
    pruned_trie.state_trie.size()
  );
  println!("  Storage tries count: {}", pruned_trie.storage_tries.len());
  println!("  Storage tries total nodes: {}", total_storage_size_after);

  // Part 9. Prune state trie
  println!("\n=== Pruning State Trie ===");
  println!("Before: {} state trie nodes", pruned_trie.state_trie.size());

  // Prune state trie to only keep paths to accessed accounts
  let accessed_accounts_hashes: std::collections::HashSet<alloy_primitives::B256> =
    accessed_accounts
      .iter()
      .map(|addr| alloy_primitives::keccak256(addr.as_slice()))
      .collect();
  pruned_trie
    .state_trie
    .prune_to_keys(&accessed_accounts_hashes);

  println!("After: {} state trie nodes", pruned_trie.state_trie.size());
  println!(
    "Removed: {} state trie nodes",
    cloned_trie.state_trie.size() - pruned_trie.state_trie.size()
  );

  let total_storage_size_final: usize = pruned_trie.storage_tries.values().map(|t| t.size()).sum();
  let total_storage_size_original: usize =
    cloned_trie.storage_tries.values().map(|t| t.size()).sum();

  println!("\nTrie statistics (after full pruning):");
  println!(
    "  State trie nodes: {} (was: {})",
    pruned_trie.state_trie.size(),
    cloned_trie.state_trie.size()
  );
  println!(
    "  Storage tries count: {} (was: {})",
    pruned_trie.storage_tries.len(),
    cloned_trie.storage_tries.len()
  );
  println!(
    "  Storage tries total nodes: {} (was: {})",
    total_storage_size_final, total_storage_size_original
  );

  // Part 10. Test pruned trie by re-executing
  println!("\n=== Testing Pruned Trie ===");
  let pruned_db = witness_db::WitnessDatabase::new(&pruned_trie, bytecode, ancestor_hashes);
  let evm_config2 = reth_proofs_core::create_mainnet_evm_config();
  let executor2 = evm_config2.executor(pruned_db);

  match executor2.execute(&current_block) {
    Ok(result2) => {
      println!("✓ Subblock executed successfully with pruned trie!");
      println!(
        "Gas used: {} (should match: {})",
        result2.gas_used, result.gas_used
      );
      if result2.gas_used == result.gas_used {
        println!("\n✅ Full witness pruning successful!");
        println!(
          "   State trie: {} → {} nodes ({:.1}% reduction)",
          cloned_trie.state_trie.size(),
          pruned_trie.state_trie.size(),
          100.0
            * (1.0 - pruned_trie.state_trie.size() as f64 / cloned_trie.state_trie.size() as f64)
        );
        println!(
          "   Storage tries count: {} → {} tries ({:.1}% reduction)",
          cloned_trie.storage_tries.len(),
          pruned_trie.storage_tries.len(),
          100.0
            * (1.0
              - pruned_trie.storage_tries.len() as f64 / cloned_trie.storage_tries.len() as f64)
        );
        println!(
          "   Storage tries nodes: {} → {} nodes ({:.1}% reduction)",
          total_storage_size_original,
          total_storage_size_final,
          100.0 * (1.0 - total_storage_size_final as f64 / total_storage_size_original as f64)
        );
      } else {
        println!("\n❌ Gas mismatch!");
      }
    }
    Err(e) => {
      println!("✗ Failed to execute with pruned trie: {:?}", e);
      println!("Pruning was too aggressive");
    }
  }
}

/// Verifies the contiguity, number of ancestor headers and extracts their hashes.
///
/// This function is used to prepare the data required for the `BLOCKHASH`
/// opcode in a stateless execution context.
///
/// It verifies that the provided `ancestor_headers` form a valid, unbroken chain leading back from
///    the parent of the `current_block`.
///
/// Note: This function becomes obsolete if EIP-2935 is implemented.
/// Note: The headers are assumed to be in ascending order.
///
/// If both checks pass, it returns a [`BTreeMap`] mapping the block number of each
/// ancestor header to its corresponding block hash.
fn compute_ancestor_hashes(
  current_block: &reth_primitives_traits::RecoveredBlock<reth_ethereum_primitives::Block>,
  ancestor_headers: &[reth_primitives_traits::SealedHeader],
) -> Result<std::collections::BTreeMap<u64, alloy_primitives::B256>, String> {
  let mut ancestor_hashes = std::collections::BTreeMap::new();

  let mut child_header = current_block.sealed_header();

  // Next verify that headers supplied are contiguous
  for parent_header in ancestor_headers.iter().rev() {
    let parent_hash = child_header.parent_hash();
    ancestor_hashes.insert(parent_header.number, parent_hash);

    if parent_hash != parent_header.hash() {
      return Err("StatelessValidationError::InvalidAncestorChain".to_string()); // Blocks must be contiguous
    }

    if parent_header.number + 1 != child_header.number {
      return Err("StatelessValidationError::InvalidAncestorChain".to_string()); // Header number should be
      // contiguous
    }

    child_header = parent_header
  }

  Ok(ancestor_hashes)
}
