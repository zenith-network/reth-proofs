use backon::Retryable;
use futures::StreamExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod alerting;

mod cli;

mod sp1;

mod worker_prepare;
mod worker_prove;

// Preparing block is only matter of seconds, no need to parellelize it.
const NUM_WORKERS_PREPARE: u8 = 1;

struct WorkerPrepareJob {
  block_number: u64,
}

impl From<u64> for WorkerPrepareJob {
  fn from(block_number: u64) -> Self {
    Self { block_number }
  }
}

struct WorkerProveJob {
  block_number: u64,
  sp1_stdin: sp1_sdk::SP1Stdin,
}

// NOTE: Since `RUST_LOG` is already used by SP1's moongate container (spawned as subprocess)
// we use different env to be able to control log levels separately.
const LOG_ENV: &str = "RUST_RSP_LOG";

#[tokio::main]
pub async fn main() -> eyre::Result<()> {
  // Initialize the environment variables.
  dotenv::dotenv().ok();

  // Default log level.
  if std::env::var(LOG_ENV).is_err() {
    unsafe {
      std::env::set_var(LOG_ENV, "info");
    }
  }

  // Initialize the logger.
  tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer())
    .with(
      tracing_subscriber::EnvFilter::from_env(LOG_ENV), // .add_directive("sp1_core_machine=warn".parse().unwrap())
                                                        // .add_directive("sp1_core_executor=warn".parse().unwrap())
                                                        // .add_directive("sp1_prover=warn".parse().unwrap()),
    )
    .init();

  // Parse the command line arguments.
  let args = <cli::Args as clap::Parser>::parse();
  let args = match args.command {
    cli::Command::GeneratePk(args) => {
      let output_path = args.output_path.clone();
      // Load RSP client binary.
      let elf = sp1_sdk::include_elf!("reth-proofs-zkvm-sp1-guest").to_vec();
      println!("Generating PK with CPU...");
      let pk = sp1::generate_pk_with_cpu(elf)?;
      sp1::store_pk_to_file(&pk, &output_path)?;
      println!("PK stored to {}", output_path.display());

      return Ok(());
    }
    cli::Command::ExtractVk(args) => {
      // Load proving key from the file.
      let proving_key_path = args.proving_key_path.clone();
      let pk = sp1::read_pk_from_file(&proving_key_path)?;

      // Extract the verifying key.
      let vk = pk.vk;
      let serialized_vk = bincode::serialize(&vk)?;

      // Write the verifying key to the output file (same as input, but different ext).
      let output_path = args.proving_key_path.with_extension("vk");
      std::fs::write(&output_path, serialized_vk)?;
      println!(
        "Verifying key extracted and saved to {}",
        output_path.display()
      );

      return Ok(());
    }
    cli::Command::PrepareBlock(args) => {
      let http_provider = reth_proofs::create_provider(args.http_rpc_url.as_str()).unwrap();

      // Prevent working with block older than 100 blocks, as getting witness for it takes ~2.5s, and gets even more slow with older block.
      let latest_block_number = alloy_provider::Provider::get_block_number(&http_provider).await?;
      if args.block_number < latest_block_number.saturating_sub(100) {
        return Err(eyre::eyre!(
          "Block number {} is too old, please use a block number at least {}",
          args.block_number,
          latest_block_number.saturating_sub(100)
        ));
      }

      // Create WokerPrepare.
      let worker = worker_prepare::WorkerPrepare::new(http_provider);

      // Get block input.
      let save_in_cache = true;
      let block_number = args.block_number;
      println!(
        "Generating input for block {} (will be stored in cache)...",
        block_number
      );
      worker.generate_input(block_number, save_in_cache).await?;
      println!("Input for block {} generated", block_number);

      return Ok(());
    }
    cli::Command::ProveBlockOffline(args) => {
      // Load block input - based on `try_loading_input_from_cache`.
      // Also get the block number from the file name.
      println!("Loading block input from {}", args.block_input.display());
      let block_number_str = args.block_input.file_stem().unwrap().to_str().unwrap();
      let block_number: u64 = block_number_str.parse()?;
      let block_input_serialized = std::fs::read(args.block_input)?;
      tracing::info!("Loaded block input for block {}", block_number);

      // Make sure serialized data is correct.
      println!("Validating block input...");
      bincode::deserialize::<reth_proofs_core::input::ZkvmInput>(&block_input_serialized)
        .expect("Failed to validate block input");
      println!("Block input validated");

      // Prepare SP1 input (essenetially copy block input to sp1 context).
      let mut sp1_stdin = sp1_sdk::SP1Stdin::new();
      sp1_stdin.write_vec(block_input_serialized);
      println!("SP1 input ready for block {}", block_number);

      // Create proving worker.
      let gpu_id = args.gpu_id;
      let pk = sp1::read_pk_from_file(&args.proving_key_path)?;
      println!("Creating WorkerProve for GPU {}", gpu_id);
      let worker = worker_prove::WorkerProve::new(gpu_id, &pk).await.unwrap();
      println!("WorkerProve created for GPU {}", gpu_id);

      // Prove the block.
      println!("Proving block {}", block_number);
      let (proving_duration, proof_bytes, cycles, _vk) = worker.prove(&sp1_stdin).await.unwrap();
      println!(
        "Block {} proved, duration: {}, proof bytes: {}, cycles: {}",
        block_number,
        proving_duration.as_secs_f32(),
        proof_bytes.len(),
        cycles
      );

      // Save the proof to a file.
      let proof_path = args.output_proof_path;
      let mut proof_file = std::fs::File::create(proof_path.clone())?;
      bincode::serialize_into(&mut proof_file, &proof_bytes)?;
      println!("Proof saved to {}", proof_path.display());

      return Ok(());
    }
    cli::Command::Run(args) => args,
  };

  tracing::info!("Running with args: {:?}", args);

  // Load pregenerated proving "key" from the file.
  // NOTE: Even called "key", it contains both PK, and ELF itself!
  let proving_key = sp1::read_pk_from_file(&args.proving_key_path)?;

  // Ethproofs clients.
  let eth_proofs_client = std::sync::Arc::new(ethproofs_api::EthProofsClient::new(
    args.eth_proofs_cluster_id,
    args.eth_proofs_endpoint,
    args.eth_proofs_api_token,
  ));
  let eth_proofs_staging_client = std::sync::Arc::new(ethproofs_api::EthProofsClient::new(
    args.eth_proofs_staging_cluster_id,
    args.eth_proofs_staging_endpoint,
    args.eth_proofs_staging_api_token,
  ));

  // Alerting client.
  let alerting_client = args
    .pager_duty_integration_key
    .map(|key| std::sync::Arc::new(alerting::AlertingClient::new(key)));

  // Token for graceful shutdown.
  let stop_token = tokio_util::sync::CancellationToken::new();
  let stop_token_clone = stop_token.clone();

  let ws = alloy_provider::WsConnect::new(args.ws_rpc_url);
  let ws_provider = alloy_provider::ProviderBuilder::new()
    .connect_ws(ws)
    .await?;
  let http_provider = reth_proofs::create_provider(args.http_rpc_url.as_str()).unwrap();

  // Subscribe to block headers.
  let subscription = alloy_provider::Provider::subscribe_blocks(&ws_provider).await?;
  let mut stream = subscription.into_stream().map(|h| h.number);

  // Queue for WorkerPrepare.
  let (job_prepare_queue_tx, job_prepare_queue_rx) =
    async_channel::bounded::<WorkerPrepareJob>(NUM_WORKERS_PREPARE as usize);

  // Queue for WorkerProve.
  let (job_prove_queue_tx, job_prove_queue_rx) =
    async_channel::bounded::<WorkerProveJob>(NUM_WORKERS_PREPARE as usize);

  // Spawn WorkerPrepare tasks.
  let mut worker_prepare_handles = Vec::new();
  for worker_id in 0..NUM_WORKERS_PREPARE {
    let http_provider = http_provider.clone();
    let job_prepare_queue_rx = job_prepare_queue_rx.clone();
    let job_prove_queue_tx = job_prove_queue_tx.clone();
    let eth_proofs_client = eth_proofs_client.clone();
    let eth_proofs_staging_client = eth_proofs_staging_client.clone();
    let alerting_client = alerting_client.clone();
    let stop_token = stop_token.clone();
    let worker_prepare_handle = tokio::task::spawn(async move {
      let worker = worker_prepare::WorkerPrepare::new(http_provider);
      while let Ok(job) = job_prepare_queue_rx.recv().await {
        let block_number = job.block_number;
        tracing::info!(
          "WorkerPrepare_{}: Processing block {}",
          worker_id,
          block_number
        );
        {
          // Before prepare hook.
          eth_proofs_staging_client.queued(block_number).await;
          if block_number % 100 == 0 {
            eth_proofs_client.queued(block_number).await;
          }
        }
        let f = || async { worker.get_input(block_number).await };
        let client_input = match f
          .retry(backon::ConstantBuilder::new().with_max_times(3))
          .notify(|err: &eyre::Report, dur: std::time::Duration| {
            println!(
              "[block {}] Retrying {:?} after {:?}",
              block_number, err, dur
            );
          })
          .await
        {
          Ok(res) => res,
          Err(err) => {
            println!("[block {}] Error while: {:?}", block_number, err);
            if let Some(alerting_client) = alerting_client.clone() {
              alerting_client
                .send_alert(format!(
                  "[{}_WorkerPrepare_{}] Getting input for block {} failed: {}",
                  args.worker_pos, worker_id, block_number, err
                ))
                .await;
            }
            continue;
          }
        };
        {
          // NOTE: Here could be after_prepare hook.
        }
        tracing::info!(
          "WorkerPrepare_{}: Client input ready for block {}",
          worker_id,
          block_number
        );

        // Wrap serialized client input in SP1 struct.
        let mut sp1_stdin = sp1_sdk::SP1Stdin::new();
        sp1_stdin.write_vec(client_input);

        // Push the job to the prove queue.
        let prove_job = WorkerProveJob {
          block_number,
          sp1_stdin,
        };
        job_prove_queue_tx.send(prove_job).await.unwrap();
        tracing::info!(
          "WorkerPrepare_{}: Job sent to WorkerProve for block {}",
          worker_id,
          block_number
        );

        // Stop if the token is cancelled.
        if stop_token.is_cancelled() {
          tracing::info!("WorkerPrepare_{}: Stopping...", worker_id);
          break;
        }
      }
    });
    worker_prepare_handles.push(worker_prepare_handle);
  }

  // Spawn WorkerProve tasks.
  let mut worker_prove_handles = Vec::new();
  let num_worker_prove = args.gpu_count;
  for worker_id in 0..num_worker_prove {
    let job_prove_queue_rx = job_prove_queue_rx.clone();
    let eth_proofs_client = eth_proofs_client.clone();
    let eth_proofs_staging_client = eth_proofs_staging_client.clone();
    let alerting_client = alerting_client.clone();
    let stop_token = stop_token.clone();
    let proving_key = proving_key.clone();
    let worker_prove_handle = tokio::task::spawn(async move {
      let gpu_id = worker_id;
      let worker = worker_prove::WorkerProve::new(gpu_id, &proving_key)
        .await
        .unwrap();
      while let Ok(job) = job_prove_queue_rx.recv().await {
        let block_number = job.block_number;
        let sp1_stdin = job.sp1_stdin;
        tracing::info!(
          "WorkerProve_{}: Processing block {}",
          worker_id,
          block_number
        );
        {
          // Before prove hook.
          eth_proofs_staging_client.proving(block_number).await;
          if block_number % 100 == 0 {
            eth_proofs_client.proving(block_number).await;
          }
        }
        let f = || async { worker.prove(&sp1_stdin).await };
        let (proving_duration, proof_bytes, cycles, vk) = match f
          .retry(backon::ConstantBuilder::new().with_max_times(3))
          .notify(|err: &eyre::Report, dur: std::time::Duration| {
            println!(
              "[block {}] retrying {:?} after {:?}",
              block_number, err, dur
            );
          })
          .await
        {
          Ok(res) => res,
          Err(err) => {
            println!("[block {}] Error: {:?}", block_number, err);
            if let Some(alerting_client) = alerting_client.clone() {
              alerting_client
                .send_alert(format!(
                  "[{}_WorkerProve_{}] Proving block {} failed: {}",
                  args.worker_pos, worker_id, block_number, err
                ))
                .await;
            }
            continue;
          }
        };
        {
          let verifier_id = sp1_sdk::HashableKey::bytes32(vk);
          // After prove hook.
          eth_proofs_staging_client
            .proved(
              &proof_bytes,
              block_number,
              cycles,
              proving_duration.as_secs_f32(),
              &verifier_id,
            )
            .await;
          if block_number % 100 == 0 {
            eth_proofs_client
              .proved(
                &proof_bytes,
                block_number,
                cycles,
                proving_duration.as_secs_f32(),
                &verifier_id,
              )
              .await;
          }
        }
        tracing::info!("WorkerProve_{}: Block {} proved", worker_id, block_number);
        tracing::info!(
          "Proving duration: {}, Proof bytes: {}, Cycles: {}",
          proving_duration.as_secs_f32(),
          proof_bytes.len(),
          cycles
        );

        // Handle stop.
        if stop_token.is_cancelled() {
          tracing::info!("WorkerProve_{}: Stopping...", worker_id);
          break;
        }
      }
    });
    worker_prove_handles.push(worker_prove_handle);
  }

  tracing::info!(
    "Latest block number in HTTP RPC: {}",
    alloy_provider::Provider::get_block_number(&http_provider).await?
  );

  // Listen to the notifications about new blocks (WebSocket).
  loop {
    tokio::select! {
      _ = tokio::signal::ctrl_c() => {
        println!("Ctrl-C received, cancelling main loop...");
        break;
      },
      block_number = stream.next() => {
        match block_number {
          Some(block_number) => {
            tracing::info!("New block {} reported by WS provider", block_number);

            // Temporarily hardcoded block number.
            // let block_number = 22187923;

            // Simple coordination logic.
            let target_worker_pos = (block_number % args.total_workers) + 1;
            if target_worker_pos != args.worker_pos {
              tracing::info!("Skipping, block {} is for worker {}", block_number, target_worker_pos);
              continue;
            }

            // Make sure the block is avaliable in the HTTP provider.
            // This could happen if WS provider is faster than HTTP one.
            let block_number_hex = block_number.into();
            while alloy_provider::Provider::get_block_by_number(&http_provider, block_number_hex).await?.is_none() {
                tracing::info!("Block {} not available in the HTTP provider, waiting...", block_number);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            tracing::info!("Block {} available in the HTTP provider", block_number);

            // Push block number to prepare queue.
            job_prepare_queue_tx.send(block_number.into()).await?;
          }
          None => {
            tracing::warn!("WS stream closed");
            break;
          }
        }
      }
    }
  }

  tracing::info!("Signaling workers to stop...");
  stop_token_clone.cancel();

  // NOTE: This does NOT really work, as workers are not checking stop condition unless new task arrives...
  //tracing::info!("Waiting for workers to finish...");
  //futures::future::join_all(worker_prepare_handles).await;

  Ok(())
}
