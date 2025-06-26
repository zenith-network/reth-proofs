#[derive(Debug, Clone)]
pub struct WorkerPrepare {
  http_provider: alloy_provider::RootProvider<alloy_provider::network::Ethereum>,
}

impl WorkerPrepare {
  pub fn new(
    http_provider: alloy_provider::RootProvider<alloy_provider::network::Ethereum>,
  ) -> Self {
    Self { http_provider }
  }

  async fn try_loading_input_from_cache(
    block_number: u64,
    validate: bool,
  ) -> eyre::Result<Option<Vec<u8>>> {
    let cache_dir = std::path::PathBuf::from("/tmp/reth_proofs_cache");
    let cache_path = cache_dir.join(format!("input/{}.bin", block_number));
    if cache_path.exists() {
      let serialized_input = std::fs::read(cache_path)?;
      // Optionally check if the input is valid.
      if validate {
        bincode::deserialize::<reth_proofs_core::input::ZkvmInput>(&serialized_input)
          .map_err(|e| eyre::eyre!("Failed to deserialize input: {}", e))?;
      }
      return Ok(Some(serialized_input));
    }

    Ok(None)
  }

  // NOTE: Client input could NOT be passed by reference as it is not `Sync`.
  async fn save_input_in_cache(block_number: u64, client_input: Vec<u8>) -> eyre::Result<()> {
    let cache_dir = std::path::PathBuf::from("/tmp/reth_proofs_cache");
    let input_folder = cache_dir.join(format!("input"));
    if !input_folder.exists() {
      std::fs::create_dir_all(&input_folder)?;
    }
    let input_path = input_folder.join(format!("{}.bin", block_number));
    std::fs::write(input_path, &client_input)
      .map_err(|e| eyre::eyre!("Failed to write input to cache: {}", e))?;

    Ok(())
  }

  pub async fn get_input(&self, block_number: u64) -> eyre::Result<Vec<u8>> {
    // Try getting input from cache.
    let validate_input = true;
    if let Some(client_input) =
      Self::try_loading_input_from_cache(block_number, validate_input).await?
    {
      tracing::info!("Client input loaded from cache for block {}", block_number);
      return Ok(client_input);
    }
    tracing::info!("Client input not found in cache for block {}", block_number);

    // If not found, generate it and store in cache.
    self.generate_input(block_number, true).await
  }

  pub async fn generate_input(
    &self,
    block_number: u64,
    save_in_cache: bool,
  ) -> eyre::Result<Vec<u8>> {
    tracing::info!("Preparing client input for block {}", block_number);
    let witness = reth_proofs::fetch_block_witness(&self.http_provider, block_number)
      .await
      .unwrap();
    let block_rpc = reth_proofs::fetch_full_block(&self.http_provider, block_number)
      .await
      .unwrap()
      .unwrap();
    let block_consensus = reth_proofs::rpc_block_to_consensus_block(block_rpc);
    let client_input =
      reth_proofs_core::input::ZkvmInput::from_offline_rpc_data(block_consensus, &witness);
    tracing::info!("Reth input prepared for block {}", block_number);

    // Serialize the input to a binary format.
    let serialized_input = bincode::serialize(&client_input)?;
    tracing::info!("Client input serialized for block {}", block_number);

    if save_in_cache {
      Self::save_input_in_cache(block_number, serialized_input.clone()).await?;
      tracing::info!("Client input saved in cache for block {}", block_number);
    }

    Ok(serialized_input)
  }
}
