use sp1_sdk::Prover;

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const GUEST_ELF: &[u8] = sp1_sdk::include_elf!("reth-proofs-zkvm-sp1-guest");

#[tokio::main]
async fn main() {
  // Prepare zkVM input from offline RPC data.
  let block_number = 22724090_u64;
  let witness = reth_proofs::load_block_witness_from_file(block_number)
    .await
    .unwrap();
  let block_rpc = reth_proofs::load_block_from_file(block_number)
    .await
    .unwrap();
  let block_consensus: alloy_consensus::Block<
    alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>,
    alloy_consensus::Header,
  > = block_rpc
    .map_transactions(|tx| alloy_consensus::EthereumTxEnvelope::from(tx))
    .into_consensus();
  let input = reth_proofs_core::input::ZkvmInput::from_offline_rpc_data(block_consensus, &witness);

  println!("Creating GPU prover...");
  let prover = sp1_sdk::client::ProverClient::builder().cuda().build();

  println!("Generating proving bundle...");
  let (pk, _vk) = prover.setup(GUEST_ELF);

  // Write input to zkVM stdin.
  let mut stdin = sp1_sdk::SP1Stdin::new();
  let input_bytes = bincode::serialize(&input).unwrap();
  stdin.write_vec(input_bytes);

  println!("Proving execution...");
  let _proof = prover.prove(&pk, &stdin).compressed().run().unwrap();

  println!("Done!");
}
