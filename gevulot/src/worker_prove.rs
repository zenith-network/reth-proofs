pub struct WorkerProve {
  gpu_client: std::sync::Arc<sp1_sdk::CudaProver>,
  pk: sp1_sdk::SP1ProvingKey,
  vk: sp1_sdk::SP1VerifyingKey,
}

impl WorkerProve {
  pub async fn new(gpu_id: u8, pregenerated_pk: &sp1_sdk::SP1ProvingKey) -> eyre::Result<Self> {
    // Try to cleanup previous instance.
    let port: u64 = 3000 + u64::from(gpu_id);
    let _ = std::process::Command::new("docker")
      .args(["rm", "-f", &format!("sp1-gpu-{}", gpu_id)])
      .output();

    // Create new moongate container (it waits until ready).
    let moongate_endpoint = sp1_cuda::MoongateServer::Local {
      visible_device_index: Some(gpu_id as u64),
      port: Some(port),
    };
    tracing::info!("Creating CUDA prover for GPU {}", gpu_id);
    // NOTE: Container logging is set to RUST_LOG level by default - and child output is printed!
    let gpu_client = std::sync::Arc::new(sp1_sdk::CudaProver::new(
      sp1_sdk::SP1Prover::new(),
      moongate_endpoint,
    ));
    tracing::info!("CUDA prover for GPU {} created", gpu_id);

    // Finally we can use pregenerated keys without setup - https://github.com/succinctlabs/sp1/pull/2264.
    let pk = pregenerated_pk.clone();
    let vk = pregenerated_pk.vk.clone();

    Ok(Self { gpu_client, pk, vk })
  }

  pub async fn prove(
    &self,
    stdin: &sp1_sdk::SP1Stdin,
  ) -> eyre::Result<(std::time::Duration, Vec<u8>, u64, sp1_sdk::SP1VerifyingKey)> {
    tracing::info!("Starting proof generation");
    let proving_start = std::time::Instant::now();
    let gpu_client_clone = self.gpu_client.clone();
    let pk_clone = self.pk.clone();
    let stdin_clone = stdin.clone();
    let (proof, cycles) = tokio::task::spawn_blocking(move || {
      gpu_client_clone
        .prove_with_cycles(&pk_clone, &stdin_clone, sp1_sdk::SP1ProofMode::Compressed)
        .map_err(|err| eyre::eyre!("{err}"))
    })
    .await
    .map_err(|err| eyre::eyre!("{err}"))??;
    tracing::info!("Proof generation finished");

    let proving_duration = proving_start.elapsed();
    let proof_bytes = bincode::serialize(&proof.proof)?;

    Ok((proving_duration, proof_bytes, cycles, self.vk.clone()))
  }
}
