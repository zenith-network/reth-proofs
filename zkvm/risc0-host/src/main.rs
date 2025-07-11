pub const RETH_PROOFS_ZKVM_RISC0_GUEST_ELF: &[u8] = include_bytes!(env!(concat!("R0_ELF_", "reth-proofs-zkvm-risc0-guest")));

use risc0_zkvm::{ExecutorEnv, default_executor};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
    .init();

  // Prepare zkVM input.
  let input_bytes = reth_proofs_zkvm_common_host::host_handler().await;

  // Execute the guest code.
  let env = ExecutorEnv::builder()
    .write(&input_bytes_len)?
    .write_slice(&input_bytes)
    .build()?;
  let exec = default_executor();
  println!("Generating traces...");
  let start = std::time::Instant::now();
  let session_info = exec.execute(env, RETH_PROOFS_ZKVM_RISC0_GUEST_ELF)?;
  let duration = start.elapsed();
  println!("Traces ready - it took {:.2} seconds", duration.as_secs_f64());
  println!("- num segments: {}", session_info.segments.len());
  println!("- total cycles: {}", session_info.cycles());

  // Proving test!
  println!("Creating local prover...");
  let prover = risc0_zkvm::LocalProver::new("local-prover");
  let env = ExecutorEnv::builder()
    .write(&input_bytes_len)?
    .write_slice(&input_bytes)
    .build()?;
  let opts = risc0_zkvm::ProverOpts::fast();
  println!("Generating proof...");
  let start = std::time::Instant::now();
  let receipt = risc0_zkvm::Prover::prove_with_opts(&prover, env, RETH_PROOFS_ZKVM_RISC0_GUEST_ELF, &opts).unwrap();
  let duration = start.elapsed();
  println!("Proof ready - it took {:.2} seconds", duration.as_secs_f64());

  Ok(())
}
