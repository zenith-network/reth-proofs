#[tokio::main]
async fn main() {
  // store_latest_block_and_witness().await;
  // debug_local_block_and_witness(22694900_u64).await;
  // reth_proofs::execute_block_reth_stateless(22694900_u64).await;
  // reth_proofs::execute_block_offline(22694900_u64)
  //   .await
  //   .unwrap();
  execute_latest_block_with_rpc().await;
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

async fn debug_local_block_and_witness(block_number: u64) {
  // Load the block and witness from files.
  let block = reth_proofs::load_block_from_file(block_number)
    .await
    .unwrap();
  let witness = reth_proofs::load_block_witness_from_file(block_number)
    .await
    .unwrap();

  // Print some information about the loaded block and witness.
  println!("Loaded block number: {}", block_number);

  // Store debug witness in file.
  let witness_debug = reth_proofs::utils::format_execution_witness(&witness);
  std::fs::write(
    format!("assets/{}_execution-witness-debug.txt", block_number),
    witness_debug,
  )
  .expect("Unable to write witness debug to file");

  // Build trie.
  let trie = reth_proofs::triedb::TrieDB::from_execution_witness(witness).unwrap();
  if trie.state_trie.size() == 0 {
    println!("Ouch! If state trie is empty, then probably you supplied invalid pre_state_root.");
    return;
  }

  // Validate that correct witness is used.
  let highest_block_number = trie
    .block_hashes
    .keys()
    .max()
    .expect("Trie should have at least one block hash");
  if (*highest_block_number + 1) != block.header.number {
    panic!(
      "Highest block in trie {} does appear to be the parent of the block {}",
      highest_block_number, block.header.number
    );
  }

  // Store the debug format of trie in file.
  let trie_debug = reth_proofs::triedb_utils::format_trie(&trie);
  std::fs::write(
    format!("assets/{}_trie-debug.txt", block_number),
    trie_debug,
  )
  .expect("Unable to write trie debug to file");
  println!(
    "Witness and trie debug saved successfully for block {}",
    block_number
  );
}

async fn execute_latest_block_with_rpc() {
  // Fetch the latest block number.
  let provider = reth_proofs::create_provider("http://130.250.187.55:8545").unwrap();
  let block_number = reth_proofs::get_last_block_number(&provider).await.unwrap();
  println!("Latest block number: {}", block_number);

  // Execute the block using RPC.
  let result =
    reth_proofs::execute_block_with_rpc("http://130.250.187.55:8545", block_number).await;
  match result {
    Ok(_) => println!("Block {} executed successfully!", block_number),
    Err(e) => eprintln!("Error executing block {}: {}", block_number, e),
  }
}
