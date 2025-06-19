use sp1_sdk::client::ProverClient;
use sp1_sdk::{Prover, SP1Stdin, include_elf};

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const GUEST_ELF: &[u8] = include_elf!("reth-proofs-zkvm-sp1-guest");

fn main() {
  let stdin = SP1Stdin::new();

  println!("Creating GPU prover...");
  let prover = ProverClient::builder().cuda().build();

  println!("Generating proving bundle...");
  let (pk, _vk) = prover.setup(GUEST_ELF);

  println!("Proving execution...");
  let _proof = prover.prove(&pk, &stdin).compressed().run().unwrap();

  println!("Done!");
}
