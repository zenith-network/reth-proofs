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

//             // Upload input to Bento.
//             let bento_input = match upload_bonsai(zkvm_input).await {
//               Ok(input) => input,
//               Err(e) => {
//                 tracing::error!("Failed to upload input for block {}: {}", block_number, e);
//                 continue;
//               }
//             };
//             let image_id_hex = bento_input.image_id_hex.clone();

            // Notify Ethproofs that we start proving.
            //ethproofs_api.proving(block_number).await;
            let start_proving_time = std::time::Instant::now();

//             // Prove the block.
//             let (receipt, cycles) = match prove_bonsai(bento_input).await {
//               Ok(receipt) => receipt,
//               Err(e) => {
//                 tracing::error!("Failed to prove block {}: {}", block_number, e);
//                 continue;
//               }
//             };

            // Stop the proving timer.
            let proving_duration = start_proving_time.elapsed();

//             // Serialize receipt.
//             let proof_bytes = match bincode::serialize(&receipt) {
//               Ok(bytes) => bytes,
//               Err(e) => {
//                 tracing::error!("Failed to serialize receipt for block {}: {}", block_number, e);
//                 continue;
//               }
//             };

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

// // Proving input, stored in Bento cluster.
// pub struct BonsaiInput {
//   image_id_hex: String,
//   input_id: String,
//   receipts_ids: Vec<String>,
// }

// // Uploads zkVM program and input to Bento.
// pub async fn upload_bonsai(zkvm_input: Vec<u8>) -> eyre::Result<BonsaiInput> {
//   // Compute the ImageID and upload the ELF binary
//   let elf = RETH_PROOFS_ZKVM_RISC0_GUEST_ELF;
//   let image_id = risc0_zkvm::compute_image_id(elf).unwrap();
//   let image_id_hex = format!("{}", image_id);

//   // Upload the image to Bonsai.
//   tracing::debug!("Uploading image - ID: {}", image_id_hex);
//   let client = bonsai_sdk::non_blocking::Client::from_env(risc0_zkvm::VERSION)?;
//   client.upload_img(&image_id_hex, elf.to_vec()).await?;

//   // Upload the zkVM input.
//   tracing::debug!("Uploading zkVM input - {} bytes", zkvm_input.len());
//   let input_id = client.upload_input(zkvm_input).await?; // Equivalent to `env.input`.

//   // No receipts to be uploaded - no assumptions.
//   let receipts_ids = vec![];

//   Ok(BonsaiInput {
//     image_id_hex,
//     input_id,
//     receipts_ids,
//   })
// }

// /// Runs proving session on Bento.
// /// Returns the receipt and the number of *total cycles* used.
// pub async fn prove_bonsai(bonsai_input: BonsaiInput) -> eyre::Result<(risc0_zkvm::Receipt, u64)> {
//   let image_id_hex = bonsai_input.image_id_hex;
//   let input_id = bonsai_input.input_id;
//   let receipts_ids = bonsai_input.receipts_ids;

//   // Start a session on the bonsai prover.
//   let start = std::time::Instant::now();
//   let session_limit: Option<u64> = None; // Equivalent to `env.session_limit`.
//   let client = bonsai_sdk::non_blocking::Client::from_env(risc0_zkvm::VERSION)?;
//   let session = client
//     .create_session_with_limit(image_id_hex, input_id, receipts_ids, false, session_limit)
//     .await?;
//   tracing::info!("Session created - ID: {}", session.uuid);

//   // The session has already been started in the executor. Poll bonsai until session is no longer running.
//   let polling_interval = if let Ok(ms) = std::env::var("BONSAI_POLL_INTERVAL_MS") {
//     std::time::Duration::from_millis(ms.parse::<u64>().unwrap())
//   } else {
//     std::time::Duration::from_secs(1)
//   };
//   let mut num_checks = 0u64;
//   let res = loop {
//     num_checks += 1;
//     tracing::debug!("Polling session result - check {}", num_checks);
//     let res = session.status(&client).await?;
//     match res.status.as_str() {
//       "RUNNING" => {
//         tokio::time::sleep(polling_interval).await;
//         continue;
//       }
//       _ => {
//         break res;
//       }
//     }
//   };
//   let duration = start.elapsed();
//   tracing::info!(
//     "Session finished - it took {:.2} seconds",
//     duration.as_secs_f64()
//   );

//   // Handle potential failure.
//   if res.status != "SUCCEEDED" {
//     tracing::error!(
//       "Bonsai prover workflow [{}] exited: {} err: {}",
//       session.uuid,
//       res.status,
//       res
//         .error_msg
//         .clone()
//         .unwrap_or("Bonsai workflow missing error_msg".into())
//     );
//     return Err(eyre::eyre!(
//       "Proving session failed: {}",
//       res.error_msg.unwrap_or("No error message provided".into())
//     ));
//   }

//   // Print stats.
//   let stats = res
//     .stats
//     .expect("Missing stats object on Bonsai status res");
//   tracing::info!(
//     "Bonsai usage: cycles: {} total_cycles: {}, segments: {}",
//     stats.cycles,
//     stats.total_cycles,
//     stats.segments,
//   );

//   // Download the receipt.
//   tracing::debug!("Downloading the receipt...");
//   let receipt_url = res
//     .receipt_url
//     .expect("API error, missing receipt on completed session");
//   let receipt_buf = client.download(&receipt_url).await?;
//   let receipt: risc0_zkvm::Receipt = bincode::deserialize(&receipt_buf)?;
//   tracing::info!("Raw receipt size: {} bytes", receipt_buf.len());

//   Ok((receipt, stats.total_cycles))
// }
