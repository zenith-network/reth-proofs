use reth_proofs_core::triedb::TrieDB;

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_triedb_from_execution_witness() {
    let mainnet_reth_nr10 = "http://130.250.187.55:8545";
    let provider = crate::create_provider(mainnet_reth_nr10).unwrap();
    let block_number = crate::get_last_block_number(&provider).await.unwrap();
    let block = crate::fetch_full_block(&provider, block_number)
      .await
      .unwrap()
      .unwrap();
    let witness = crate::fetch_block_witness(&provider, block_number)
      .await
      .unwrap();

    let block = crate::rpc_block_to_consensus_block(block);
    let _trie_db = TrieDB::from_execution_witness(witness, &block)
      .expect("Failed to create TrieDB from execution witness");
  }
}
