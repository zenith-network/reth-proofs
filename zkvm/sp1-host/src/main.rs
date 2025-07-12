use sp1_sdk::Prover;

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const GUEST_ELF: &[u8] = sp1_sdk::include_elf!("reth-proofs-zkvm-sp1-guest");

#[tokio::main]
async fn main() {
  // Prepare zkVM input.
  let input_bytes = reth_proofs_zkvm_common_host::host_handler().await;

  println!("Creating GPU prover...");
  let prover = sp1_sdk::client::ProverClient::builder().cuda().build();

  println!("Generating proving bundle...");
  let (pk, _vk) = prover.setup(GUEST_ELF);

  // Write input to zkVM stdin.
  let mut stdin = sp1_sdk::SP1Stdin::new();
  stdin.write_vec(input_bytes);

  // Execute the program using the `ProverClient.execute` method, without generating a proof.
  //let (_, report) = prover.execute(GUEST_ELF, &stdin).run().unwrap();

  println!("Proving execution...");
  let start = std::time::Instant::now();
  let (_proof_values, cycles) = prover
    .prove_with_cycles(&pk, &stdin, sp1_sdk::SP1ProofMode::Compressed)
    .unwrap();
  let duration = start.elapsed();
  println!("Proof generated with {} cycles.", cycles);
  println!(
    "Proof generation time: {:.2} seconds",
    duration.as_secs_f64()
  );
}
