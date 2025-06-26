use backon::Retryable;
use futures::StreamExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    cli::Command::Run(args) => args,
  };

  // Load pregenerated proving "key" from the file.
  // NOTE: Even called "key", it contains both PK, and ELF itself!
  let proving_key = sp1::read_pk_from_file(&args.proving_key_path)?;

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
          // TODO: Before prove hook.
        }
        let f = || async { worker.prove(&sp1_stdin).await };
        let (proving_duration, proof_bytes, cycles, _vk) = match f
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
            continue;
          }
        };
        {
          // TODO: After prove hook.
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
