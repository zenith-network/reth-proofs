#![no_main]
sp1_zkvm::entrypoint!(main);

pub fn main() {
  println!("cycle-tracker-start: compute");
  reth_proofs_core::create_mainnet_evm_config();
  println!("cycle-tracker-end: compute");
}
