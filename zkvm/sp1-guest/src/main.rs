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

  // 3. Sealing and validating ancestor headers - 27K cycles.
  ancestor_headers.seal_and_validate();
}
