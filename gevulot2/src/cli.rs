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
  /// Run main orchestration loop.
  Run(RunArgs),

  /// Prepare a block for offline proving.
  PrepareBlock(PrepareBlockArgs),

  /// Prove a single given block.
  ProveBlock(ProveBlockArgs),
}

#[derive(Debug, Clone, clap::Parser)]
pub struct RunArgs {
  /// The HTTP rpc url used to fetch data about the block.
  #[clap(long, env)]
  pub http_rpc_url: url::Url,
  
  /// The WS rpc url used to fetch data about the block.
  #[clap(long, env)]
  pub ws_rpc_url: url::Url,

  /// ETH proofs endpoint.
  #[clap(long, env)]
  pub ethproofs_api_url: String,

  /// ETH proofs API token.
  #[clap(long, env)]
  pub ethproofs_api_token: String,

  /// ETH proofs cluster ID.
  #[clap(long, env)]
  pub ethproofs_cluster_id: u64,
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

  /// Path to the output file.
  #[clap(long, default_value = "zkvm_input.bin")]
  pub output_path: std::path::PathBuf,
}

/// Args specific to the `prove-block` command.
#[derive(Debug, clap::Parser)]
pub struct ProveBlockArgs {
  /// Path to the block file, eg. `zkvm_input.bin`.
  #[clap(long)]
  pub zkvm_input: std::path::PathBuf,

  /// Path to the output proof file.
  #[clap(long, default_value = "proof.bin")]
  pub output_proof_path: std::path::PathBuf,
}
