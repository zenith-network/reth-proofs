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

  /// Run main orchestration loop.
  Run(RunArgs),
}

/// Args specific to the `generate-pk` command.
#[derive(Debug, clap::Parser)]
pub struct GeneratePkArgs {
  /// Path where the proving key should be saved.
  #[clap(long, default_value = "pk.bin")]
  pub output_path: std::path::PathBuf,
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
}
