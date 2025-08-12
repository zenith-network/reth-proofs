/// The arguments for the cli.
#[derive(Debug, clap::Parser)]
#[command(name = "myapp")]
#[command(about = "My CLI with subcommands", long_about = None)]
pub struct Args {
  #[clap(subcommand)]
  pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
  /// Generate a proving key and store it to a file.
  GeneratePk(GeneratePkArgs),

  /// Extracts a verifying key from PK bundle.
  ExtractVk(ExtractVkArgs),

  /// Run main orchestration loop.
  Run(RunArgs),

  /// Prove a single given block.
  /// Useful for reproducing SP1 bugs.
  ProveBlockOffline(ProveBlockOfflineArgs),

  /// Prepare a block for offline proving.
  PrepareBlock(PrepareBlockArgs),
}

/// Args specific to the `generate-pk` command.
#[derive(Debug, clap::Parser)]
pub struct GeneratePkArgs {
  /// Path where the proving key should be saved.
  #[clap(long, default_value = "pk.bin")]
  pub output_path: std::path::PathBuf,
}

/// Args specific to the `extract-vk` command.
#[derive(Debug, clap::Parser)]
pub struct ExtractVkArgs {
  /// Path to the proving key file.
  /// Pregenerated Proving Key (includes the ELF and Verification Key).
  #[clap(long)]
  pub proving_key_path: std::path::PathBuf,
}

#[derive(Debug, Clone, clap::Parser)]
pub struct RunArgs {
  /// The HTTP rpc url used to fetch data about the block.
  #[clap(long, env)]
  pub http_rpc_url: url::Url,

  /// The WS rpc url used to fetch data about the block.
  #[clap(long, env)]
  pub ws_rpc_url: url::Url,

  /// Pregenerated Proving Key (includes the ELF and Verification Key).
  #[clap(long, env)]
  pub proving_key_path: std::path::PathBuf,

  /// Number of GPUs available, affecting the number of prove workers.
  #[clap(long, env, default_value_t = 1)]
  pub gpu_count: u8,

  /// Number of all workers in the Gevulot pool.
  #[clap(long, env)]
  pub total_workers: u64,

  /// Assignment of current worker - value between <1, workers>.
  /// Used to split the work between multiple instances.
  #[clap(long, env)]
  pub worker_pos: u64,

  /// ETH proofs endpoint.
  #[clap(long, env)]
  pub eth_proofs_endpoint: String,

  /// ETH proofs API token.
  #[clap(long, env)]
  pub eth_proofs_api_token: String,

  /// Optional ETH proofs cluster ID.
  #[clap(long, default_value_t = 1)]
  pub eth_proofs_cluster_id: u64,

  /// ETH proofs staging endpoint.
  #[clap(long, env)]
  pub eth_proofs_staging_endpoint: String,

  /// ETH proofs staging API token.
  #[clap(long, env)]
  pub eth_proofs_staging_api_token: String,

  /// Optional ETH proofs staging cluster ID.
  #[clap(long, default_value_t = 1)]
  pub eth_proofs_staging_cluster_id: u64,

  /// PagerDuty integration key.
  #[clap(long, env)]
  pub pager_duty_integration_key: Option<String>,
}

/// Args specific to the `prove-block-offline` command.
#[derive(Debug, clap::Parser)]
pub struct ProveBlockOfflineArgs {
  /// Path to the block file, eg. `12345.bin`.
  /// Base name of the file MUST be the block number.
  #[clap(long)]
  pub block_input: std::path::PathBuf,

  /// Path to the proving key file.
  #[clap(long)]
  pub proving_key_path: std::path::PathBuf,

  /// GPU id for proof container.
  #[clap(long, default_value_t = 0)]
  pub gpu_id: u8,

  /// Path to the output proof file.
  #[clap(long, default_value = "proof.bin")]
  pub output_proof_path: std::path::PathBuf,
}

/// Args specific to the `prepare-block` command.
#[derive(Debug, clap::Parser)]
pub struct PrepareBlockArgs {
  /// Block number.
  #[clap(long)]
  pub block_number: u64,

  /// The HTTP rpc url used to fetch data about the block.
  #[clap(long, env)]
  pub http_rpc_url: url::Url,
}
