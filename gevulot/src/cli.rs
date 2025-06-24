use clap::{Parser, Subcommand};

/// The arguments for the cli.
#[derive(Debug, Parser)]
#[command(name = "myapp")]
#[command(about = "My CLI with subcommands", long_about = None)]
pub struct Args {
  #[clap(subcommand)]
  pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
  /// Generate a proving key and store it to a file.
  GeneratePk(GeneratePkArgs),

  /// Run main orchestration loop.
  Run(RunArgs),
}

/// Args specific to the `generate-pk` command.
#[derive(Debug, Parser)]
pub struct GeneratePkArgs {
  /// Path where the proving key should be saved.
  #[clap(long, default_value = "pk.bin")]
  pub output_path: std::path::PathBuf,
}

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
  /// Pregenerated Proving Key (includes the ELF and Verification Key).
  #[clap(long, env)]
  pub proving_key_path: std::path::PathBuf,
}
