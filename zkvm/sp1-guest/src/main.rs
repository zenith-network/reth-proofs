#![no_main]
sp1_zkvm::entrypoint!(main);

pub fn main() {
  println!("cycle-tracker-start: compute");
  reth_proofs_core::create_mainnet_chainspec();
  println!("cycle-tracker-end: compute");
}
