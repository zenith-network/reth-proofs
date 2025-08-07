pub const RETH_PROOFS_ZKVM_RISC0_GUEST_ELF: &[u8] =
  include_bytes!(env!(concat!("R0_ELF_", "reth-proofs-zkvm-risc0-guest")));

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
    .init();

  // Prepare input data.
  let input_bytes = reth_proofs_zkvm_common_host::host_handler().await;
  let input_bytes_len = input_bytes.len() as u32;

  // Wrap input into zkVM format (raw frame).
  let mut input = vec![];
  input.extend_from_slice(&input_bytes_len.to_le_bytes());
  input.extend_from_slice(&input_bytes);

  // Compute the ImageID and upload the ELF binary
  let elf = RETH_PROOFS_ZKVM_RISC0_GUEST_ELF;
  let image_id = risc0_zkvm::compute_image_id(elf)?;
  let image_id_hex = format!("{}", image_id);

  // Upload the image to Bonsai.
  println!("Uploading image - ID: {}", image_id_hex);
  let client = bonsai_sdk::non_blocking::Client::from_env(risc0_zkvm::VERSION)?;
  client.upload_img(&image_id_hex, elf.to_vec()).await?;

  // Upload the zkVM input.
  println!("Uploading zkVM input - {} bytes", input.len());
  let input_id = client.upload_input(input).await?; // Equivalent to `env.input`.

  // No receipts to be uploaded - no assumptions.
  let receipts_ids = vec![];

  // Start a session on the bonsai prover.
  let start = std::time::Instant::now();
  let session_limit: Option<u64> = None; // Equivalent to `env.session_limit`.
  let session = client.create_session_with_limit(
      image_id_hex,
      input_id,
      receipts_ids,
      false,
      session_limit,
  ).await?;
  println!("Session created - ID: {}", session.uuid);

  // The session has already been started in the executor. Poll bonsai until session is no longer running.
  let polling_interval = if let Ok(ms) = std::env::var("BONSAI_POLL_INTERVAL_MS") {
    std::time::Duration::from_millis(ms.parse::<u64>().unwrap())
  } else {
    std::time::Duration::from_secs(1)
  };
  let mut num_checks = 0u64;
  let res = loop {
    num_checks += 1;
    println!("Polling session result - check {}", num_checks);
    let res = session.status(&client).await?;
    match res.status.as_str() {
      "RUNNING" => {
        tokio::time::sleep(polling_interval).await;
        continue;
      },
      _ => {
        break res;
      }
    }
  };
  let duration = start.elapsed();
  println!(
    "Session finished - it took {:.2} seconds",
    duration.as_secs_f64()
  );

  // Handle potential failure.
  if res.status != "SUCCEEDED" {
    println!("Bonsai prover workflow [{}] exited: {} err: {}",
        session.uuid, res.status, res.error_msg.unwrap_or("Bonsai workflow missing error_msg".into()));
    return Ok(());
  }

  // Print stats.
  let stats = res
    .stats
    .expect("Missing stats object on Bonsai status res");
  println!(
  "Bonsai usage: cycles: {} total_cycles: {}, segments: {}",
    stats.cycles,
    stats.total_cycles,
    stats.segments,
  );
  
  // Download the receipt.
  let receipt_url = res
    .receipt_url
    .expect("API error, missing receipt on completed session");
  let receipt_buf = client.download(&receipt_url).await?;
  let _receipt: risc0_zkvm::Receipt = bincode::deserialize(&receipt_buf)?;        
  println!("Raw receipt size: {} bytes", receipt_buf.len());

  Ok(())
}
