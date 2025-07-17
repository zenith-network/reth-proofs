pub async fn host_handler() -> Vec<u8> {
  // Prepare zkVM input from offline RPC data.
  let block_number = 22830000_u64;
  let witness = reth_proofs::load_block_witness_from_file(block_number)
    .await
    .unwrap();
  let block_rpc = reth_proofs::load_block_from_file(block_number)
    .await
    .unwrap();
  let block_consensus = reth_proofs::rpc_block_to_consensus_block(block_rpc);
  let input = reth_proofs_core::input::ZkvmInput::from_offline_rpc_data(block_consensus, &witness);

  // Serialize the input to bytes.
  let input_bytes = bincode::serialize(&input).unwrap();

  input_bytes
}

pub async fn host_alt_handler() -> Vec<u8> {
  // Prepare zkVM input from offline RPC data.
  let block_number = 22830000_u64;
  let witness = reth_proofs::load_block_witness_from_file(block_number)
    .await
    .unwrap();
  let block_rpc = reth_proofs::load_block_from_file(block_number)
    .await
    .unwrap();
  let block_consensus = reth_proofs::rpc_block_to_consensus_block(block_rpc);
  let current_block = reth_proofs_core::CurrentBlock { body: block_consensus };
  let input = reth_proofs_core::input_alt::ZkvmAltInput {
    block: current_block,
    witness,
  };

  // Serialize the input to bytes.
  let input_bytes = bincode::serialize(&input).unwrap();

  input_bytes
}
