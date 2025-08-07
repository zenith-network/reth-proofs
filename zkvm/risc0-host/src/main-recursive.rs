pub const RETH_PROOFS_ZKVM_RISC0_GUEST_ELF: &[u8] =
  include_bytes!(env!(concat!("R0_ELF_", "reth-proofs-zkvm-risc0-guest")));

use risc0_zkvm::{
  ExecutorEnv, ExecutorImpl, LocalProver, Prover, ProverOpts, ProverServer, ReceiptClaim,
  SimpleSegmentRef, SuccinctReceipt, VerifierContext, default_executor, get_prover_server,
};
use std::rc::Rc;
use std::time::Instant;

/// Join many succinct receipts into one, timing every join.
fn join_many(
  mut receipts: Vec<SuccinctReceipt<ReceiptClaim>>,
  prover: Rc<dyn ProverServer>,
) -> SuccinctReceipt<ReceiptClaim> {
  let mut join_idx = 0usize;

  while receipts.len() > 1 {
    let mut joined_receipts = Vec::new();
    for pair in receipts.chunks(2) {
      if pair.len() < 2 {
        joined_receipts.push(pair[0].clone());
        break;
      }

      let start = Instant::now();
      let joined = prover.join(&pair[0], &pair[1]).unwrap();
      let dur = start.elapsed();
      println!("  • join #{:03}: {:.2} s", join_idx, dur.as_secs_f64());
      join_idx += 1;
      joined_receipts.push(joined);
    }
    receipts = joined_receipts;
  }

  receipts.remove(0)
}

const MAX_PO2: usize = 21;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
    .init();

  // Prepare zkVM input.
  let input_bytes = reth_proofs_zkvm_common_host::host_handler().await;

  // ───────────── 0. REFERENCE (non‑parallel) RUN ─────────────
  {
    let env_ref = ExecutorEnv::builder()
      .segment_limit_po2(MAX_PO2 as u32)
      .write_frame(&input_bytes)
      .build()?;
    let exec = default_executor();

    println!("Generating non-parallel segments/traces …");
    let start = Instant::now();
    let session_info = exec.execute(env_ref, RETH_PROOFS_ZKVM_RISC0_GUEST_ELF)?;
    let dur = start.elapsed();
    println!(
      "Reference segments/traces ready in {:.2} s",
      dur.as_secs_f64()
    );
    println!("- num segments: {}", session_info.segments.len());
    println!("- total cycles: {}", session_info.cycles());
  }

  // MONOLITHIC / NON‑PARALLEL PROVING (LocalProver, ProverOpts::fast)
  {
    println!("\n=== Non‑parallel monolithic proof ===");
    let prover = LocalProver::new("local-prover");

    let env_full = ExecutorEnv::builder()
      .segment_limit_po2(MAX_PO2 as u32)
      .write_frame(&input_bytes)
      .build()?;
    let opts_full = ProverOpts::fast();

    println!("Generating single‑segment proof (CPU) …");
    let t0 = Instant::now();
    let receipt = Prover::prove_with_opts(
      &prover,
      env_full,
      RETH_PROOFS_ZKVM_RISC0_GUEST_ELF,
      &opts_full,
    )?;
    println!("Proof ready in {:.2} s", t0.elapsed().as_secs_f64());
    println!("Receipt size: {} bytes", receipt.receipt.seal_size());
  }

  // ───────────── 1. SEGMENTED RUN  ─────────────
  let env = ExecutorEnv::builder()
    .segment_limit_po2(MAX_PO2 as u32)
    .write_frame(&input_bytes)
    .build()?;

  println!("Generating parallel segments/traces …");
  let start = Instant::now();
  let session = ExecutorImpl::from_elf(env, RETH_PROOFS_ZKVM_RISC0_GUEST_ELF)?
    .run_with_callback(|seg| Ok(Box::new(SimpleSegmentRef::new(seg))))?;
  let dur = start.elapsed();
  println!(
    "Segments ready in {:.2} s — {} segment(s), total cycles (incl. overhead): {}",
    dur.as_secs_f64(),
    session.segments.len(),
    session.total_cycles
  );

  // ───────────── 2. PROVE EACH SEGMENT ─────────────
  println!("Creating local prover …");
  let opts = ProverOpts::from_max_po2(MAX_PO2);
  let ctx = VerifierContext::default();
  let prover = get_prover_server(&opts)?;

  let mut receipts = Vec::with_capacity(session.segments.len());

  for (idx, seg_ref) in session.segments.iter().enumerate() {
    let segment = seg_ref.resolve()?;

    // ‣ Prove
    let start = Instant::now();
    let seg_receipt = prover.prove_segment(&ctx, &segment)?;
    let prove_dur = start.elapsed();
    println!(
      "  • segment #{:03}: prove   {:.2} s",
      idx,
      prove_dur.as_secs_f64()
    );

    // ‣ Lift
    let start = Instant::now();
    let succinct = prover.lift(&seg_receipt)?;
    let lift_dur = start.elapsed();
    println!(
      "  • segment #{:03}: lift    {:.2} s",
      idx,
      lift_dur.as_secs_f64()
    );

    receipts.push(succinct);
  }
  println!("Finished proving {} segment(s).", receipts.len());

  // ───────────── 3. JOIN RECEIPTS ─────────────
  println!("Joining receipts …");
  let final_receipt = join_many(receipts, prover);
  println!(
    "All joins complete — final receipt ready (seal size: {}).",
    final_receipt.seal_size()
  );

  Ok(())
}
