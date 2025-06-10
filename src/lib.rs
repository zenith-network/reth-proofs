use alloy_provider::RootProvider;

pub fn create_provider(http_rpc_url: &str) -> alloy_provider::RootProvider<alloy_provider::network::Ethereum> {
  let url = http_rpc_url.try_into().unwrap();
  let provider = RootProvider::new_http(url);

  provider
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_create_provider() {
    let mock_url = "https://google.com";
    create_provider(mock_url);
  }
}
