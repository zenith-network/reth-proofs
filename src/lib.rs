#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("Invalid URL: {0}")]
  InvalidUrl(String),
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
