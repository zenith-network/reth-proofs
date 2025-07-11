#![no_main]

extern crate alloc;

sp1_zkvm::entrypoint!(main);

fn main() {
  // Get input from the host.
  let buffer = sp1_zkvm::io::read_vec();

  // Run guest logic.
  reth_proofs_zkvm_common_guest::guest_handler(&buffer);
}
