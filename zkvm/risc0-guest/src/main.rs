#![no_main]

extern crate alloc;

risc0_zkvm::guest::entry!(main);

fn main() {
  // Get input from the host.
  let buffer = risc0_zkvm::guest::env::read_frame();

  // Run guest logic.
  reth_proofs_zkvm_common_guest::guest_handler(&buffer);
}
