#![no_std]

// It is used in the `BasicBlockExecutor` as "strategy factory", implementing `ConfigureEvm` trait.
// Measured SP1 performance:
// - no precompiles - 501M cycles (deserialization took 64M)
// - enabled `tiny-keccak` feature in `alloy_primitives` + added `tiny-keccak` precompile - 501M cycles (deserialization took 64M)
// - enabled `sha3-keccak` feature in `alloy_primitives` + added `sha3` precompile - 136M cycles (deserialization took 64M)
// - enabled `sha3-keccak` feature in alloy_primitives + no precompiles - 501M cycles (deserialization took 64M)
pub fn create_mainnet_evm_config() -> reth_ethereum::evm::EthEvmConfig {
  reth_ethereum::evm::EthEvmConfig::mainnet()
}

/// Creates an Ethereum mainnet chainspec.
/// Measured SP1 performance:
/// - 32K cycles (hardfork creation took 21K cycles, could be optimized)
///
/// NOTE: Chainspec is not serializable; genesis is.
///
/// NOTE: We are using chainspec from RSP, which is NOT a 100% Ethereum chainspec.
/// See the "real" mainnet chainspec here:
/// - https://github.com/paradigmxyz/reth/blob/127595e23079de2c494048d0821ea1f1107eb624/crates/chainspec/src/spec.rs#L88-L114
///
/// TODO: See if we can construct more Ethereum-like chainspec, without adding too many cycles.
pub fn create_mainnet_chainspec() -> reth_chainspec::ChainSpec {
  let mainnet = reth_chainspec::ChainSpec {
    chain: reth_chainspec::Chain::mainnet(),
    genesis: Default::default(),
    genesis_header: Default::default(),
    paris_block_and_final_difficulty: Default::default(),
    hardforks: reth_chainspec::EthereumHardfork::mainnet().into(),
    deposit_contract: Default::default(),
    base_fee_params: reth_chainspec::BaseFeeParamsKind::Constant(
      reth_chainspec::BaseFeeParams::ethereum(),
    ),
    prune_delete_limit: 20000,
    blob_params: Default::default(),
  };

  mainnet
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_create_mainnet_evm_config() {
    let config = create_mainnet_evm_config();
    assert_eq!(
      reth_ethereum::chainspec::EthChainSpec::chain_id(&config.chain_spec()),
      1
    );
  }
}
