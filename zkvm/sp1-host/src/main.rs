use sp1_sdk::include_elf;

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const GUEST_ELF: &[u8] = include_elf!("reth-proofs-zkvm-sp1-guest");

fn main() {
  // TODO
}
