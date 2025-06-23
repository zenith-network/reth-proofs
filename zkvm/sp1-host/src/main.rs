use reth_proofs::load_block_witness_from_file;
use sp1_sdk::client::ProverClient;
use sp1_sdk::{Prover, SP1Stdin, include_elf};

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const GUEST_ELF: &[u8] = include_elf!("reth-proofs-zkvm-sp1-guest");

#[tokio::main]
async fn main() {
  let mut stdin = SP1Stdin::new();

  // 1. Write ancestor headers to stdin.
  let witness = load_block_witness_from_file(22724090_u64).await.unwrap();
  let ancestor_headers = reth_proofs_core::AncestorHeaders::from_execution_witness(&witness);
  let ancestor_headers_bytes = bincode::serialize(&ancestor_headers).unwrap();
  stdin.write_vec(ancestor_headers_bytes);

  // 2. Write current block to stdin.
  let block_rpc = reth_proofs::load_block_from_file(22724090_u64)
    .await
    .unwrap();
  let block_consensus: alloy_consensus::Block<
    alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>,
    alloy_consensus::Header,
  > = block_rpc
    .map_transactions(|tx| alloy_consensus::EthereumTxEnvelope::from(tx))
    .into_consensus();
  let block_consensus_bincode = reth_proofs_core::CurrentBlock {
    body: block_consensus.clone(),
  };
  let block_bytes = bincode::serialize(&block_consensus_bincode).unwrap();
  stdin.write_vec(block_bytes);

  // 3. Write witness state to stdin.
  let witness = load_block_witness_from_file(22724090_u64).await.unwrap();
  let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;
  let etherum_state: reth_proofs_core::EthereumState =
    reth_proofs_core::EthereumState::from_execution_witness(&witness, pre_state_root);
  let ethereum_state_bytes = bincode::serialize(&etherum_state).unwrap();
  stdin.write_vec(ethereum_state_bytes);

  println!("Creating GPU prover...");
  let prover = ProverClient::builder().cuda().build();

  println!("Generating proving bundle...");
  let (pk, _vk) = prover.setup(GUEST_ELF);

  println!("Proving execution...");
  let _proof = prover.prove(&pk, &stdin).compressed().run().unwrap();

  println!("Done!");
}
