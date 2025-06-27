use risc0_zkvm::guest::env;

extern crate alloc;

fn main() {
  // 1. Creating a mainnet EVM config - 32K cycles -> 97K cycles
  let start = env::cycle_count();
  let chainspec = reth_proofs_core::create_mainnet_chainspec();
  let chainspec_arc = alloc::sync::Arc::new(chainspec);
  let evm_config = reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc.clone());
  let end = env::cycle_count();
  eprintln!("creating_mainnet_evm_config: {}", end - start);

  // 2. Reading input data from stdin - 80.4M cycles -> 103.9M cycles
  let start = env::cycle_count();
  let len: usize = env::read();
  let mut buffer = vec![0u8; len];
  env::read_slice(&mut buffer);
  let zkvm_input = bincode::deserialize::<reth_proofs_core::input::ZkvmInput>(&buffer).unwrap();
  let ancestor_headers = zkvm_input.ancestor_headers;
  let current_block = zkvm_input.current_block;
  let ethereum_state = zkvm_input.ethereum_state;
  let bytecodes = zkvm_input.bytecodes;
  let end = env::cycle_count();
  eprintln!("reading_input_data: {}", end - start);

  // 4. Sealing and validating all headers - 41K cycles -> 284K cycles
  let start = env::cycle_count();
  let block_hashes = ancestor_headers.seal_and_validate(&current_block);
  let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;
  let end = env::cycle_count();
  eprintln!("sealing_and_validating_headers: {}", end - start);

  // 6. Validating state trie - 31.5M cycles -> 260M cycles
  let start = env::cycle_count();
  reth_proofs_core::validate_state_trie(&ethereum_state.state_trie, pre_state_root);
  let end = env::cycle_count();
  eprintln!("validating_state_trie: {}", end - start);

  // 7. Validating storage tries - 47.4M cycles -> 400M cycles
  let start = env::cycle_count();
  reth_proofs_core::validate_storage_tries(
    &ethereum_state.state_trie,
    &ethereum_state.storage_tries,
  )
  .unwrap();
  let end = env::cycle_count();
  eprintln!("validating_storage_tries: {}", end - start);

  // 9. Building bytecode map - 42K cycles -> 533M cycles
  let start = env::cycle_count();
  let bytecode_by_hash = bytecodes.build_map();
  let end = env::cycle_count();
  eprintln!("building_bytecode_map: {}", end - start);

  // 10. Prepare database for EVM execution.
  let mut trie_db = reth_proofs_core::triedb::TrieDB {
    state_trie: ethereum_state.state_trie,
    storage_tries: ethereum_state.storage_tries,
    bytecode_by_hash,
    block_hashes,
  };
  let db = reth_proofs_core::triedb::wrap_into_database(&trie_db);

  // 11. Create block executor - 0 cycles.
  let block_executor =
    reth_ethereum::evm::primitives::execute::BasicBlockExecutor::new(&evm_config, db);

  // 12. Recover block signatures - 15M cycles -> 3111M cycles (over 50% of total cycles!!!)
  let start = env::cycle_count();
  let recovered_block = current_block.recover_senders();
  let end = env::cycle_count();
  eprintln!("recovering_senders: {}", end - start);

  // 13. Execute block - 284.3M cycles -> 1127M cycles
  let start = env::cycle_count();
  let output =
    reth_ethereum::evm::primitives::execute::Executor::execute(block_executor, &recovered_block)
      .unwrap();
  let end = env::cycle_count();
  eprintln!("executing_block: {}", end - start);

  // 14. Validate block post execution - 7.8M cycles -> 91M cycles
  let start = env::cycle_count();
  reth_proofs_core::validate_block_post_execution(
    &recovered_block,
    chainspec_arc.as_ref(),
    &output,
  );
  let end = env::cycle_count();
  eprintln!("validating_post_execution: {}", end - start);

  // 15. Get hashed post state - 3.3M cycles -> 27.3M cycles
  let start = env::cycle_count();
  let hashed_post_state = reth_proofs_core::get_hashed_post_state(&output);
  let end = env::cycle_count();
  eprintln!("getting_hashed_post_state: {}", end - start);

  // 16. Apply state updates - 21.2M cycles -> 186M cycles
  let start = env::cycle_count();
  trie_db.update(&hashed_post_state);
  let end = env::cycle_count();
  eprintln!("apply_trie_updates: {}", end - start);

  // 17. Compute new state root and verify - 15.5M cycles -> 170M cycles
  let start = env::cycle_count();
  let new_state_root = trie_db.compute_state_root();
  let expected_root = recovered_block.header().state_root;
  if new_state_root != expected_root {
    panic!(
      "New state root does not match expected root: {:?} != {:?}",
      new_state_root, expected_root
    );
  }
  let end = env::cycle_count();
  eprintln!("computing_state_root: {}", end - start);
}
