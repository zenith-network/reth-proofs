#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("Invalid URL: {0}")]
  InvalidUrl(String),

  #[error("RPC error: {0}")]
  RPC(String),
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

pub async fn fetch_block(
  provider: &alloy_provider::RootProvider,
  block_number: u64,
) -> Result<Option<alloy_rpc_types_eth::Block>, Error> {
  let block_number = block_number.into();
  let block = alloy_provider::Provider::get_block_by_number(&provider, block_number)
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
    fetch_block(&provider, block_number).await.unwrap();
  }

  #[tokio::test]
  async fn test_fetch_block_witness() {
    let mainnet_reth_nr10 = "http://130.250.187.55:8545";
    let provider = create_provider(mainnet_reth_nr10).unwrap();

    // NOTE: We fetch witness for the latest block, as every single older one is slower to compute.
    let block_number = get_last_block_number(&provider).await.unwrap();

    let witness = fetch_block_witness(&provider, block_number).await;
    assert!(
      witness.is_ok(),
      "Failed to fetch block witness: {:?}",
      witness.err()
    );
  }
}
