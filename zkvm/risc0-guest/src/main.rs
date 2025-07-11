#![no_main]

extern crate alloc;

risc0_zkvm::guest::entry!(main);

fn main() {
  // Get input from the host.
  let buffer_len: usize = risc0_zkvm::guest::env::read();
  let mut buffer = alloc::vec::Vec::<u8>::with_capacity(buffer_len);
  risc0_zkvm::guest::env::read_slice(&mut buffer);

  // Run guest logic.
  reth_proofs_zkvm_common_guest::guest_handler(&buffer);
}
