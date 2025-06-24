#![no_main]
sp1_zkvm::entrypoint!(main);

extern crate alloc;

// For mainnet block 22724090, we got 552M cycles in SP1.
pub fn main() {
  // 1. Creating a mainnet EVM config - 32K cycles.
  let chainspec = reth_proofs_core::create_mainnet_chainspec();
  let chainspec_arc = alloc::sync::Arc::new(chainspec);
  let evm_config = reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc.clone());

  // 2. Reading ancestor headers from stdin - 17K cycles.
  let buffer = sp1_zkvm::io::read_vec();
  let ancestor_headers =
    bincode::deserialize::<reth_proofs_core::AncestorHeaders>(&buffer).unwrap();

  // 3. Reading current block from stdin - 3.3M cycles.
  let buffer = sp1_zkvm::io::read_vec();
  let current_block = bincode::deserialize::<reth_proofs_core::CurrentBlock>(&buffer).unwrap();

  // 4. Sealing and validating all headers - 41K cycles.
  let block_hashes = ancestor_headers.seal_and_validate(&current_block);
  let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;

  // 5. Reading ethereum state from stdin - 77.1M cycles.
  let buffer = sp1_zkvm::io::read_vec();
  let ethereum_state = bincode::deserialize::<reth_proofs_core::EthereumState>(&buffer).unwrap();

  // 6. Validating state trie - 31.5M cycles.
  reth_proofs_core::validate_state_trie(&ethereum_state.state_trie, pre_state_root);

  // 7. Validating storage tries - 47.4M cycles.
  reth_proofs_core::validate_storage_tries(
    &ethereum_state.state_trie,
    &ethereum_state.storage_tries,
  )
  .unwrap();

  // 8. Reading bytecodes from stdin - 3K cycles.
  let buffer = sp1_zkvm::io::read_vec();
  let bytecodes = bincode::deserialize::<reth_proofs_core::Bytecodes>(&buffer).unwrap();

  // 9. Building bytecode map - 42K cycles.
  let bytecode_by_hash = bytecodes.build_map();

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

  // 12. Recover block signatures - 15M cycles.
  let recovered_block = current_block.recover_senders();

  // 13. Execute block - 284.3M cycles.
  let output =
    reth_ethereum::evm::primitives::execute::Executor::execute(block_executor, &recovered_block)
      .unwrap();

  // 14. Validate block post execution - 7.8M cycles.
  reth_proofs_core::validate_block_post_execution(
    &recovered_block,
    chainspec_arc.as_ref(),
    &output,
  );

  // 15. Get hashed post state - 3.3M cycles.
  let hashed_post_state = reth_proofs_core::get_hashed_post_state(&output);

  // 16. Apply state updates - 21.2M cycles.
  trie_db.update(&hashed_post_state);

  // 17. Compute new state root and verify - 15.5M cycles.
  let new_state_root = trie_db.compute_state_root();
  let expected_root = recovered_block.header().state_root;
  if new_state_root != expected_root {
    panic!(
      "New state root does not match expected root: {:?} != {:?}",
      new_state_root, expected_root
    );
  }
}
