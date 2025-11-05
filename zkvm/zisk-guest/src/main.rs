#![no_main]

mod kzg;

ziskos::entrypoint!(main);

fn main() {
  // Get input from the host
  let buffer = ziskos::read_input();

  // Install custom crypto provider (KZG) for revm precompiles.
  revm::precompile::interface::install_crypto(kzg::CryptoProvider);

  // Run guest logic.
  // NOTE: We skip 4 first bytes, as this was Risc0 frame header.
  reth_proofs_zkvm_common_guest::guest_handler(&buffer[4..]);
}
