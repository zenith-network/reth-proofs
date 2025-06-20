use reth_proofs::load_block_witness_from_file;
use sp1_sdk::client::ProverClient;
use sp1_sdk::{Prover, SP1Stdin, include_elf};

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const GUEST_ELF: &[u8] = include_elf!("reth-proofs-zkvm-sp1-guest");

#[tokio::main]
async fn main() {
  let mut stdin = SP1Stdin::new();
  let witness = load_block_witness_from_file(22724090_u64).await.unwrap();
  let ancestor_headers = reth_proofs::ancestor_headers_from_execution_witness(&witness);
  let ancestor_headers_bytes = bincode::serialize(&ancestor_headers).unwrap();
  stdin.write_vec(ancestor_headers_bytes);

  println!("Creating GPU prover...");
  let prover = ProverClient::builder().cuda().build();

  println!("Generating proving bundle...");
  let (pk, _vk) = prover.setup(GUEST_ELF);

  println!("Proving execution...");
  let _proof = prover.prove(&pk, &stdin).compressed().run().unwrap();

  println!("Done!");
}
