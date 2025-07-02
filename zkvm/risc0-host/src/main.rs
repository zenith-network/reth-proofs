use reth_proofs_zkvm_risc0_methods::RETH_PROOFS_ZKVM_RISC0_GUEST_ELF;
use risc0_zkvm::{ExecutorEnv, default_executor};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
    .init();

  // Prepare zkVM input from offline RPC data.
  let block_number = 22815400_u64;
  let witness = reth_proofs::load_block_witness_from_file(block_number)
    .await
    .unwrap();
  let block_rpc = reth_proofs::load_block_from_file(block_number)
    .await
    .unwrap();
  let block_consensus = reth_proofs::rpc_block_to_consensus_block(block_rpc);
  let input = reth_proofs_core::input::ZkvmInput::from_offline_rpc_data(block_consensus, &witness);
  let input_bytes = bincode::serialize(&input).unwrap();
  let input_bytes_len = input_bytes.len();

  // Execute the guest code.
  let env = ExecutorEnv::builder()
    .write(&input_bytes_len)?
    .write_slice(&input_bytes)
    .build()?;
  let exec = default_executor();
  exec.execute(env, RETH_PROOFS_ZKVM_RISC0_GUEST_ELF)?;
  println!("Done!");

  // Proving test!
  println!("Creating local prover...");
  let prover = risc0_zkvm::LocalProver::new("local-prover");
  println!("Generating proof... {}", std::time::UNIX_EPOCH.elapsed().unwrap().as_millis());
  let receipt = risc0_zkvm::Prover::prove(&prover, env, RETH_PROOFS_ZKVM_RISC0_GUEST_ELF).unwrap();
  println!("Proof ready! {}", std::time::UNIX_EPOCH.elapsed().unwrap().as_millis());

  Ok(())
}
