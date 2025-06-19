mod mpt;
pub mod triedb;
pub mod triedb_utils;
pub mod utils;

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("Invalid URL: {0}")]
  InvalidUrl(String),

  #[error("RPC error: {0}")]
  RPC(String),

  #[error("Failed to recover block: {0}")]
  BlockRecovery(String),

  #[error("Failed to build trie DB: {0}")]
  TrieDB(String),

  #[error("Failed to execute block: {0}")]
  Execution(String),

  #[error("Validation error: {0}")]
  Validation(String),
}

pub fn create_provider(
  http_rpc_url: &str,
) -> Result<alloy_provider::RootProvider<alloy_provider::network::Ethereum>, Error> {
  let url = http_rpc_url
    .try_into()
    .map_err(|e| Error::InvalidUrl(format!("{}", e)))?;
  let provider = alloy_provider::RootProvider::new_http(url);

  Ok(provider)
}

pub async fn get_last_block_number(provider: &alloy_provider::RootProvider) -> Result<u64, Error> {
  let block_number = alloy_provider::Provider::get_block_number(&provider)
    .await
    .map_err(|e| Error::RPC(e.to_string()))?;

  Ok(block_number)
}

/// Fetches block with *full* transactions data.
pub async fn fetch_full_block(
  provider: &alloy_provider::RootProvider,
  block_number: u64,
) -> Result<Option<alloy_rpc_types_eth::Block>, Error> {
  let block_number = block_number.into();
  let block = alloy_provider::Provider::get_block_by_number(&provider, block_number)
    .full()
    .await
    .map_err(|e| Error::RPC(e.to_string()))?;

  Ok(block)
}

pub async fn fetch_block_witness(
  provider: &alloy_provider::RootProvider,
  block_number: u64,
) -> Result<alloy_rpc_types_debug::ExecutionWitness, Error> {
  let block_number = block_number.into();
  let witness = alloy_provider::ext::DebugApi::debug_execution_witness(&provider, block_number)
    .await
    .map_err(|e| Error::RPC(e.to_string()))?;

  Ok(witness)
}

// TODO: Consider if this implementation is done in a sane way.
pub fn recover_block(
  block: alloy_rpc_types_eth::Block,
) -> Result<
  reth_primitives_traits::RecoveredBlock<alloy_consensus::Block<reth_ethereum::TransactionSigned>>,
  Error,
> {
  let block: alloy_consensus::Block<reth_ethereum::TransactionSigned> = block
    .map_transactions(|tx| alloy_consensus::TxEnvelope::from(tx).into())
    .into_consensus();
  let recovered_block = reth_primitives_traits::RecoveredBlock::try_recover(block)
    .map_err(|e| Error::BlockRecovery(format!("{}", e)))?;

  Ok(recovered_block)
}

pub async fn prepare_block_trie_db(
  provider: &alloy_provider::RootProvider,
  block: &alloy_rpc_types_eth::Block,
) -> Result<triedb::TrieDB, Error> {
  let block_number = block.header.number;
  let witness = fetch_block_witness(provider, block_number).await?;

  let trie_db =
    triedb::TrieDB::from_execution_witness(witness).map_err(|e| Error::TrieDB(format!("{}", e)))?;

  Ok(trie_db)
}

pub async fn execute_block_offline(block_number: u64) -> Result<(), Error> {
  let block = load_block_from_file(block_number).await?;
  let witness = load_block_witness_from_file(block_number).await?;

  execute_block(block, witness)
    .await
    .map(|_| ())
    .map_err(|e| Error::Execution(format!("Failed to execute block: {}", e)))
}

pub async fn execute_block_with_rpc(http_rpc_url: &str, block_number: u64) -> Result<(), Error> {
  let provider = create_provider(http_rpc_url)?;
  let block = fetch_full_block(&provider, block_number)
    .await?
    .ok_or_else(|| Error::RPC("Block not found".to_string()))?;
  let witness = fetch_block_witness(&provider, block_number).await?;

  execute_block(block, witness)
    .await
    .map(|_| ())
    .map_err(|e| Error::Execution(format!("Failed to execute block: {}", e)))
}

// TODO: Split into core functions.
pub async fn execute_block(
  block: alloy_rpc_types_eth::Block,
  witness: alloy_rpc_types_debug::ExecutionWitness,
) -> Result<reth_execution_types::BlockExecutionOutput<reth_ethereum::Receipt>, Error> {
  let config = reth_proofs_core::create_mainnet_evm_config();
  let mut trie_db =
    triedb::TrieDB::from_execution_witness(witness).map_err(|e| Error::TrieDB(format!("{}", e)))?;
  let db = revm::database::WrapDatabaseRef(&trie_db);
  let block_executor =
    reth_ethereum::evm::primitives::execute::BasicBlockExecutor::new(config.clone(), db);

  // Before proceeding, make sure that block has full tx data, not just hashes.
  // Otherwise executor silently executes 0 tx.
  if !block.transactions.is_full() {
    return Err(Error::RPC(
      "Tx missing - make sure that you fetch block with tx included".to_string(),
    ));
  }

  let recovered_block: reth_primitives_traits::RecoveredBlock<
    alloy_consensus::Block<reth_ethereum::TransactionSigned>,
  > = recover_block(block)?;

  let output =
    reth_ethereum::evm::primitives::execute::Executor::execute(block_executor, &recovered_block)
      .map_err(|e| Error::Execution(format!("{}", e)))?;

  // Validate the block post execution.
  let chain_spec = reth_chainspec::MAINNET.as_ref();
  reth_ethereum_consensus::validate_block_post_execution(
    &recovered_block,
    chain_spec,
    &output.result.receipts,
    &output.result.requests,
  )
  .map_err(|e| Error::Validation(format!("Block validation failed after execution: {}", e)))?;

  // Apply execution changes and compute the new state root.
  let hashed_post_state = reth_trie::HashedPostState::from_bundle_state::<reth_trie::KeccakKeyHasher>(
    &output.state.state,
  );
  trie_db.update(&hashed_post_state);
  let new_state_root = trie_db.compute_state_root();

  // Validate the state root after execution.
  let expected_root = recovered_block.header().state_root;
  if new_state_root != expected_root {
    return Err(Error::Validation(format!(
      "State root mismatch after execution: expected {}, got {}",
      expected_root, new_state_root
    )));
  }

  Ok(output)
}

// NOTE: Useful for debugging `reth-stateless` behavior.
pub async fn execute_offline_block_with_reth_stateless(block_number: u64) {
  let current_block = load_block_from_file(block_number).await.unwrap();
  let mut witness = load_block_witness_from_file(block_number).await.unwrap();
  witness.keys.clear();
  let chain_spec = std::sync::Arc::new(reth_chainspec::MAINNET.as_ref().clone());
  let evm_config = reth_proofs_core::create_mainnet_evm_config();
  reth_stateless::validation::stateless_validation(
    current_block.into(),
    witness,
    chain_spec,
    evm_config,
  )
  .map_err(|e| Error::Validation(format!("Stateless validation failed: {}", e)))
  .unwrap();
}

pub async fn save_block_in_file(block: &alloy_rpc_types_eth::Block) -> Result<(), Error> {
  let block_number = block.header.number;
  let file_name = format!("assets/{}_full-block.json", block_number);
  let pretty_json = serde_json::to_string_pretty(&block)
    .map_err(|e| Error::RPC(format!("Failed to serialize block to JSON: {}", e)))?;
  std::fs::write(&file_name, pretty_json).map_err(|e| {
    Error::RPC(format!(
      "Failed to write block to file {}: {}",
      file_name, e
    ))
  })?;

  Ok(())
}

pub async fn load_block_from_file(block_number: u64) -> Result<alloy_rpc_types_eth::Block, Error> {
  let file_name = format!("assets/{}_full-block.json", block_number);
  let content = std::fs::read_to_string(file_name)
    .map_err(|e| Error::RPC(format!("Failed to read block from file: {}", e)))?;

  let block: alloy_rpc_types_eth::Block = serde_json::from_str(&content)
    .map_err(|e| Error::RPC(format!("Failed to parse block JSON: {}", e)))?;

  Ok(block)
}

pub async fn save_block_witness_in_file(
  witness: &alloy_rpc_types_debug::ExecutionWitness,
  block_number: u64,
) -> Result<(), Error> {
  let file_name = format!("assets/{}_execution-witness.json", block_number);
  let pretty_json = serde_json::to_string_pretty(&witness)
    .map_err(|e| Error::RPC(format!("Failed to serialize witness to JSON: {}", e)))?;
  std::fs::write(&file_name, pretty_json)
    .map_err(|e| Error::RPC(format!("Failed to write witness to file: {}", e)))?;

  Ok(())
}

pub async fn load_block_witness_from_file(
  block_number: u64,
) -> Result<alloy_rpc_types_debug::ExecutionWitness, Error> {
  let file_name = format!("assets/{}_execution-witness.json", block_number);
  let content = std::fs::read_to_string(file_name)
    .map_err(|e| Error::RPC(format!("Failed to read witness from file: {}", e)))?;

  let witness: alloy_rpc_types_debug::ExecutionWitness = serde_json::from_str(&content)
    .map_err(|e| Error::RPC(format!("Failed to parse witness JSON: {}", e)))?;

  Ok(witness)
}

#[cfg(test)]
mod tests {
  use super::*;

  // NOTE: This MUST be a Reth archive node.
  const MAINNET_RETH_RPC_EL: &str = "http://130.250.187.55:8545";

  #[test]
  fn test_create_provider() {
    let mock_url = "https://google.com";
    create_provider(mock_url).unwrap();
  }

  #[test]
  fn test_create_provider_with_invalid_url() {
    let invalid_mock_url = "foo-bar";
    let maybe_provider = create_provider(invalid_mock_url);
    assert!(maybe_provider.is_err());
  }

  #[tokio::test]
  async fn test_get_last_block_number() {
    let provider = create_provider(MAINNET_RETH_RPC_EL).unwrap();

    let block_number = get_last_block_number(&provider).await.unwrap();
    assert!(block_number > 0);
  }

  #[tokio::test]
  async fn test_fetch_block() {
    let provider = create_provider(MAINNET_RETH_RPC_EL).unwrap();

    let block_number: u64 = 1;
    fetch_full_block(&provider, block_number).await.unwrap();
  }

  #[tokio::test]
  async fn test_fetch_block_witness() {
    let provider = create_provider(MAINNET_RETH_RPC_EL).unwrap();

    // NOTE: We fetch witness for the latest block, as every single older one is slower to compute.
    let block_number = get_last_block_number(&provider).await.unwrap();

    let witness = fetch_block_witness(&provider, block_number).await;
    assert!(
      witness.is_ok(),
      "Failed to fetch block witness: {:?}",
      witness.err()
    );
  }

  #[tokio::test]
  async fn test_recover_block() {
    let provider = create_provider(MAINNET_RETH_RPC_EL).unwrap();
    let block_number = get_last_block_number(&provider).await.unwrap();
    let block = fetch_full_block(&provider, block_number)
      .await
      .unwrap()
      .unwrap();

    let recovered_block = recover_block(block);
    assert!(
      recovered_block.is_ok(),
      "Failed to recover block: {:?}",
      recovered_block.err()
    );
  }

  #[tokio::test]
  async fn test_execute_block() {
    let provider = create_provider(MAINNET_RETH_RPC_EL).unwrap();
    let block_number = get_last_block_number(&provider).await.unwrap();

    let result = execute_block_with_rpc(MAINNET_RETH_RPC_EL, block_number).await;
    assert!(
      result.is_ok(),
      "Failed to execute block: {:?}",
      result.err()
    );
  }
}
