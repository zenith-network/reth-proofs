#![no_main]

ziskos::entrypoint!(main);

fn main() {
  // Get input from the host
  let buffer = ziskos::read_input();

  // Run guest logic.
  // NOTE: We skip 4 first bytes, as this was Risc0 frame header.
  reth_proofs_zkvm_common_guest::guest_handler(&buffer[4..]);
}
