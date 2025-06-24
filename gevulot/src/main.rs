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

  Ok(())
}
