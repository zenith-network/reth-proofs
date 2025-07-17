// Input for reth-stateless crate.

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ZkvmAltInput {
  pub block: crate::CurrentBlock,
  // NOTE: Passing raw witness to zkVM does NOT make sense - it would be
  // much better to pass serialized tries and validate them.
  pub witness: reth_stateless::ExecutionWitness,
}
