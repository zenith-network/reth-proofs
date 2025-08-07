use futures::StreamExt;

mod cli;

#[tokio::main]
pub async fn main() -> eyre::Result<()> {
  tracing::info!("Yo! Starting 'Gevulot Ethereum Prover' - recursive proving with Bento!");

  // Initialize the environment variables.
  dotenv::dotenv().ok();

  // Initialize the logger.
  // NOTE: Default log level is "info".
  let filter = tracing_subscriber::filter::EnvFilter::from_default_env()
    .add_directive("info".parse().unwrap());
  tracing_subscriber::fmt()
    .with_env_filter(filter)
    .init();

  // Parse the command line arguments.
  let args = <cli::Args as clap::Parser>::parse();

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
            tracing::info!("New block {} reported by WS provider", block_number);

            // Simple coordination logic.
            if block_number % 100 != 0 {
              tracing::info!("Skipping block {} - not our target", block_number);
              continue;
            }
            tracing::info!("Processing block {}", block_number);

            // Fetch block and witness.
            tracing::info!("Fetching RPC data for block {}", block_number);
            let witness = reth_proofs::fetch_block_witness(&http_provider, block_number)
              .await
              .unwrap();
            let block_rpc = reth_proofs::fetch_full_block(&http_provider, block_number)
              .await
              .unwrap()
              .unwrap();

            tracing::info!("Preparing zkVM input for block {}", block_number);
            let mut zkvm_input = vec![];
            {
              // 1) Prepare client input.
              let block_consensus = reth_proofs::rpc_block_to_consensus_block(block_rpc);
              let client_input = reth_proofs_core::input::ZkvmInput::from_offline_rpc_data(block_consensus, &witness);

              // 2) Serialize the input to bytes.
              let input_bytes = bincode::serialize(&client_input).unwrap();
              let input_bytes_len = input_bytes.len() as u32;

              // 3 Wrap input into zkVM format (raw frame).
              zkvm_input.extend_from_slice(&input_bytes_len.to_le_bytes());
              zkvm_input.extend_from_slice(&input_bytes);
            }
            tracing::info!("zkVM input prepared for block {}", block_number);
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
