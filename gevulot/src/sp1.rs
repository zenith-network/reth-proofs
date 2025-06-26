// NOTE: If you need VK, just get it with `pk.vk`.
pub fn generate_pk_with_cpu(elf: Vec<u8>) -> eyre::Result<sp1_sdk::SP1ProvingKey> {
  // Create CPU prover
  let cpu_client = sp1_sdk::ProverClient::builder().cpu().build();

  // Setup PK and VK
  let (pk, _vk) = sp1_sdk::Prover::setup(&cpu_client, &elf);
  Ok(pk)
}

pub fn store_pk_to_file<P: AsRef<std::path::Path>>(
  pk: &sp1_sdk::SP1ProvingKey,
  path: &P,
) -> eyre::Result<()> {
  let pk_bytes = bincode::serialize(pk)?;
  std::fs::write(path, pk_bytes)?;
  Ok(())
}

pub fn read_pk_from_file<P: AsRef<std::path::Path>>(
  path: &P,
) -> eyre::Result<sp1_sdk::SP1ProvingKey> {
  let pk_bytes = std::fs::read(path)?;
  let pk: sp1_sdk::SP1ProvingKey = bincode::deserialize(&pk_bytes)?;
  Ok(pk)
}
