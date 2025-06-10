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
}
