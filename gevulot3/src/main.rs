use futures::StreamExt;

mod cli;
mod zisk;

#[tokio::main]
pub async fn main() -> eyre::Result<()> {
  tracing::info!("Yo! Starting 'Gevulot Ethereum Prover' - recursive proving with Bento!");

  // Initialize the environment variables.
  dotenv::dotenv().ok();

  // Initialize the logger.
  // NOTE: Default log level is "info".
  let filter = tracing_subscriber::filter::EnvFilter::from_default_env()
    .add_directive("info".parse().unwrap());
  tracing_subscriber::fmt().with_env_filter(filter).init();

  // Parse the command line arguments.
  let args = <cli::Args as clap::Parser>::parse();
  let args = match args.command {
    cli::Command::Run(run_args) => run_args,
  };

  // Ethproofs client.
  let ethproofs_api = ethproofs_api::EthProofsClient::new(
    args.ethproofs_cluster_id,
    args.ethproofs_api_url,
    args.ethproofs_api_token,
  );

  // Configuration for ZisK proving coordination.
  let zisk_coordinator_url = args.zisk_coordinator_url.to_string();
  let zisk_compute_units = args.zisk_compute_units;
  let zisk_webhook_port = args.zisk_webhook_port;

  // Configure RPCs - both HTTP and WS.
  let ws = alloy_provider::WsConnect::new(args.ws_rpc_url);
  let ws_provider = alloy_provider::ProviderBuilder::new()
    .connect_ws(ws)
    .await?;
  let http_provider = reth_proofs::create_provider(args.http_rpc_url.as_str()).unwrap();

  // Subscribe to block headers.
  let subscription = alloy_provider::Provider::subscribe_blocks(&ws_provider).await?;
  let mut stream = subscription.into_stream().map(|h| h.number);

  tracing::info!(
    "Latest block number in HTTP RPC: {}",
    alloy_provider::Provider::get_block_number(&http_provider).await?
  );

  // Start long-running webhook server for ZisK callbacks.
  // CAUTION: Make sure coordinator was started with `--webhook-url 'http://[THIS_MACHINE_IP]:{}/webhook/{$job_id}'`.
  let (zisk_webhook_server, zisk_webhook_server_handle) =
    zisk::start_webhook_server(zisk_webhook_port)
      .await
      .expect("Failed to start ZisK webhook server");
  tracing::info!("ZisK webhook server started on port {}", zisk_webhook_port);

  // Listen to the notifications about new blocks (WebSocket).
  loop {
    tokio::select! {
      // _ = tokio::signal::ctrl_c() => {
      //   println!("Ctrl-C received, cancelling main loop...");
      //   break;
      // },
      block_number = stream.next() => {
        match block_number {
          Some(block_number) => {
            tracing::debug!("New block {} reported by WS provider", block_number);

            // Simple coordination logic.
            if block_number % 100 != 0 {
              tracing::debug!("Skipping block {} - not our target", block_number);
              continue;
            }
            tracing::info!("Processing block {}", block_number);
            let start_total_time = std::time::Instant::now();

            // Notify Ethproofs that we started processing.
            //ethproofs_api.queued(block_number).await;

            // Prepare input.
            let zkvm_input = match prepare_input(block_number, &http_provider).await {
              Ok(input) => input,
              Err(e) => {
                tracing::error!("Failed to prepare input for block {}: {}", block_number, e);
                continue;
              }
            };

            // Notify Ethproofs that we start proving.
            //ethproofs_api.proving(block_number).await;
            let start_proving_time = std::time::Instant::now();

            // Submit proving job.
            let input_file_name = format!("{block_number}.bin");
            let job_id = match zisk::submit_proof_job(&zisk_coordinator_url, zkvm_input, &input_file_name, zisk_compute_units).await {
              Ok(id) => id,
              Err(e) => {
                tracing::error!("Failed to submit proof job for block {}: {}", block_number, e);
                continue;
              }
            };

            // Register the job with the webhook server to receive the result.
            let zisk_webhook_result_rx = zisk_webhook_server.register_job(job_id.clone()).await;

            // Wait for proving result.
            let proving_result = match zisk_webhook_result_rx.await {
              Ok(payload) => payload,
              Err(_e) => {
                tracing::error!("Unable to receive webhook result - channel closed?");
                continue;
              }
            };
            let zisk::WebhookPayload { proof: maybe_proof, error: maybe_error, duration_ms, executed_steps, .. } = proving_result;
            let proof_bytes = match (maybe_proof, maybe_error) {
              (_, Some(error)) => {
                tracing::error!("Failed to prove block {}: {:?}", block_number, error);
                continue;
              }
              (None, None) => {
                tracing::error!("Empty proof for block {}", block_number);
                continue;
              }
              (Some(proof), None) => proof,
            };
            let cycles = executed_steps.unwrap_or(0); // TODO: This SHOULD be an error.

            // Stop the proving timer.
            let proving_duration = start_proving_time.elapsed();

            // Upload receipt to the Ethproofs endpoint.
            //ethproofs_api.proved(
            //  &proof_bytes,
            //  block_number,
            //  cycles,
            //  proving_duration.as_secs_f32(),
            //  &image_id_hex,
            //).await;

            let duration_total_time = start_total_time.elapsed();
            tracing::info!(
              "Total time for block {}: {:.2} seconds",
              block_number,
              duration_total_time.as_secs_f64()
            );
          }
          None => {
            tracing::warn!("WS stream closed");
            break;
          }
        }
      }
    }
  }

  // Shutdown the webhook server.
  tracing::info!("Shutting down webhook server");
  zisk_webhook_server_handle.abort();

  Ok(())
}

pub async fn prepare_input(
  block_number: u64,
  http_provider: &alloy_provider::RootProvider,
) -> eyre::Result<Vec<u8>> {
  // Fetch block and witness.
  tracing::info!("Fetching RPC data for block {}", block_number);
  let witness = reth_proofs::fetch_block_witness(&http_provider, block_number)
    .await
    .unwrap();
  let block_rpc = reth_proofs::fetch_full_block(&http_provider, block_number)
    .await
    .unwrap()
    .unwrap();
  tracing::info!(
    "Stats of block {}: gas used = {}, tx count = {}",
    block_number,
    block_rpc.header.gas_used,
    block_rpc.transactions.len()
  );

  // Dump block and witness as JSON to /tmp. Useful for debugging.
  if std::env::var("DUMP_RAW_INPUT").is_ok() {
    tracing::info!("Dumping block and witness JSON to /tmp");
    let block_json = serde_json::to_string_pretty(&block_rpc).unwrap();
    std::fs::write(
      format!("/tmp/reth_proofs_{}_block.json", block_number),
      block_json,
    )
    .unwrap();
    let witness_json = serde_json::to_string_pretty(&witness).unwrap();
    std::fs::write(
      format!("/tmp/reth_proofs_{}_witness.json", block_number),
      witness_json,
    )
    .unwrap();
  }

  prepare_zkvm_input(witness, block_rpc)
}

fn prepare_zkvm_input(
  witness: reth_proofs::ExecutionWitness,
  block_rpc: reth_proofs::RpcBlock,
) -> eyre::Result<Vec<u8>> {
  let block_number = block_rpc.header.number;
  tracing::debug!("Preparing zkVM input for block {}", block_number);
  let mut zkvm_input = vec![];
  {
    // 1) Prepare client input.
    let block_consensus = reth_proofs::rpc_block_to_consensus_block(block_rpc);
    let client_input =
      reth_proofs_core::input::ZkvmInput::from_offline_rpc_data(block_consensus, &witness);

    // 2) Serialize the input to bytes.
    let input_bytes = bincode::serialize(&client_input).unwrap();
    let input_bytes_len = input_bytes.len() as u32;

    // 3 Wrap input into zkVM format (raw frame).
    zkvm_input.extend_from_slice(&input_bytes_len.to_le_bytes());
    zkvm_input.extend_from_slice(&input_bytes);
  }
  tracing::info!("zkVM input prepared for block {}", block_number);

  Ok(zkvm_input)
}
