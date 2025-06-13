#[tokio::main]
async fn main() {
  store_latest_block_and_witness().await;
}

async fn store_latest_block_and_witness() {
  // Fetch the latest block and its witness.
  let provider = reth_proofs::create_provider("http://130.250.187.55:8545").unwrap();
  let block_number = reth_proofs::get_last_block_number(&provider).await.unwrap();
  let block = reth_proofs::fetch_full_block(&provider, block_number)
    .await
    .unwrap()
    .unwrap();
  let witness = reth_proofs::fetch_block_witness(&provider, block_number)
    .await
    .unwrap();

  // Store them in files.
  reth_proofs::save_block_in_file(&block).await.unwrap();
  reth_proofs::save_block_witness_in_file(&witness, block_number)
    .await
    .unwrap();
  println!("Block {} and its witness saved successfully!", block_number);
}
