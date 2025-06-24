mod cli;

#[tokio::main]
pub async fn main() -> eyre::Result<()> {
  // Parse the command line arguments.
  let args = <cli::Args as clap::Parser>::parse();
  let args = match args.command {
    cli::Command::GeneratePk(args) => {
      let output_path = args.output_path.clone();
      // Load RSP client binary.
      let elf = sp1_sdk::include_elf!("reth-proofs-zkvm-sp1-guest").to_vec();
      // println!("Generating PK with CPU...");
      // let pk = sp1::generate_pk_with_cpu(elf)?;
      // sp1::store_pk_to_file(&pk, &output_path)?;
      // println!("PK stored to {}", output_path.display());
      return Ok(());
    }
    cli::Command::Run(args) => args,
  };

  Ok(())
}
