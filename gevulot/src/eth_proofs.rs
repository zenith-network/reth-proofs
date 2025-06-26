#[derive(Debug, Clone)]
pub struct EthProofsClient {
  cluster_id: u64,
  endpoint: String,
  api_token: String,
  client: reqwest_middleware::ClientWithMiddleware,
}

impl EthProofsClient {
  pub fn new(cluster_id: u64, endpoint: String, api_token: String) -> Self {
    let retry_policy =
      reqwest_retry::policies::ExponentialBackoff::builder().build_with_max_retries(3);
    let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
      .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
        retry_policy,
      ))
      .build();

    Self {
      cluster_id,
      endpoint,
      api_token,
      client,
    }
  }

  pub async fn queued(&self, block_number: u64) {
    let json = serde_json::json!({
        "block_number": block_number,
        "cluster_id": self.cluster_id,
    });

    let this = self.clone();

    // Spawn another task to avoid retries to impact block execution
    tokio::spawn(async move {
      let response = this
        .client
        .post(format!("{}/proofs/queued", this.endpoint))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", this.api_token))
        .json(&json)
        .send()
        .await
        .map_err(|e| eyre::eyre!(e))
        .and_then(|r| r.error_for_status().map_err(|e| eyre::eyre!(e)));

      if let Err(err) = response {
        tracing::error!("Failed to report proof queuing: {}", err)
      }
    });
  }

  pub async fn proving(&self, block_number: u64) {
    let json = serde_json::json!({
        "block_number": block_number,
        "cluster_id": self.cluster_id,
    });
    let this = self.clone();

    // Spawn another task to avoid retries to impact block execution
    tokio::spawn(async move {
      let response = this
        .client
        .post(format!("{}/proofs/proving", this.endpoint))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", this.api_token))
        .json(&json)
        .send()
        .await
        .map_err(|e| eyre::eyre!(e))
        .and_then(|r| r.error_for_status().map_err(|e| eyre::eyre!(e)));

      if let Err(err) = response {
        tracing::error!("Failed to report proof proving: {}", err)
      }
    });
  }

  pub async fn proved(
    &self,
    proof_bytes: &[u8],
    block_number: u64,
    num_opcodes: u64,
    elapsed: f32,
    vk: &sp1_sdk::SP1VerifyingKey,
  ) {
    let json = serde_json::json!({
        "proof": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, proof_bytes),
        "block_number": block_number,
        "proving_cycles": num_opcodes,
        "proving_time": (elapsed * 1000.0) as u64,
        "verifier_id": sp1_sdk::HashableKey::bytes32(vk),
        "cluster_id": self.cluster_id,
    });

    let this = self.clone();

    // Spawn another task to avoid retries to impact block execution
    tokio::spawn(async move {
      // Submit proof to the API

      let response = this
        .client
        .post(format!("{}/proofs/proved", this.endpoint))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", this.api_token))
        .json(&json)
        .send()
        .await
        .map_err(|e| eyre::eyre!(e))
        .and_then(|r| r.error_for_status().map_err(|e| eyre::eyre!(e)));

      if let Err(err) = response {
        tracing::error!("Failed to report proof proving: {}", err)
      }
    });
  }
}
