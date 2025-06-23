#![no_main]
sp1_zkvm::entrypoint!(main);

extern crate alloc;

pub fn main() {
  // 1. Creating a mainnet EVM config - 32K cycles.
  // let chainspec = reth_proofs_core::create_mainnet_chainspec();
  // let chainspec_arc = alloc::sync::Arc::new(chainspec);
  // reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc);

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

  // 6. Validating storage tries - 47.4M cycles.
  reth_proofs_core::validate_storage_tries(
    &ethereum_state.state_trie,
    &ethereum_state.storage_tries,
  )
  .unwrap();
}
