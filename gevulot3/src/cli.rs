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

  /// ZisK coordinator URL.
  #[clap(long, env)]
  pub zisk_coordinator_url: url::Url,

  /// ZisK compute units - decides how many workers will be used.
  #[clap(long, env)]
  pub zisk_compute_units: u32,

  /// ZisK webhook port - on this port we will be waiting for proving callback request.
  #[clap(long, env)]
  pub zisk_webhook_port: u16,
}
