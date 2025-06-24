use futures::StreamExt;

mod cli;

mod sp1;

#[tokio::main]
pub async fn main() -> eyre::Result<()> {
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
  let proving_key_bytes = std::fs::read(&args.proving_key_path)?;
  let proving_key: sp1_sdk::SP1ProvingKey = bincode::deserialize(&proving_key_bytes)?;

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
