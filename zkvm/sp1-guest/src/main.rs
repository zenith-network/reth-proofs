#![no_main]
sp1_zkvm::entrypoint!(main);

extern crate alloc;

pub fn main() {
  // 1. Creating a mainnet EVM config - 32K cycles.
  let chainspec = reth_proofs_core::create_mainnet_chainspec();
  let chainspec_arc = alloc::sync::Arc::new(chainspec);
  let evm_config = reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc);

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
  let trie_db = reth_proofs_core::triedb::TrieDB {
    state_trie: ethereum_state.state_trie,
    storage_tries: ethereum_state.storage_tries,
    bytecode_by_hash,
    block_hashes,
  };
  let db = reth_proofs_core::triedb::wrap_into_database(&trie_db);

  // 11. Create block executor - 0 cycles.
  let block_executor =
    reth_ethereum::evm::primitives::execute::BasicBlockExecutor::new(&evm_config, db);

  // 12. Recover block signatures - 3.3B cycles.
  let recovered_block = current_block.recover_senders();
}
