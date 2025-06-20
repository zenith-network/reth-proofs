#![no_main]
sp1_zkvm::entrypoint!(main);

extern crate alloc;

pub fn main() {
  let chainspec = reth_proofs_core::create_mainnet_chainspec();
  let chainspec_arc = alloc::sync::Arc::new(chainspec);
  reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc);
}
