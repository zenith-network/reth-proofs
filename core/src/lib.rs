#![no_std]

extern crate alloc;

pub mod input;
pub mod input_alt;
pub mod triedb;

pub use reth_trie_sp1_zkvm::mpt;

use alloy_signer::k256::ecdsa::signature::hazmat::PrehashVerifier;

// It is used in the `BasicBlockExecutor` as "strategy factory", implementing `ConfigureEvm` trait.
// Measured SP1 performance:
// - no precompiles - 501M cycles (deserialization took 64M)
// - enabled `tiny-keccak` feature in `alloy_primitives` + added `tiny-keccak` precompile - 501M cycles (deserialization took 64M)
// - enabled `sha3-keccak` feature in `alloy_primitives` + added `sha3` precompile - 136M cycles (deserialization took 64M)
// - enabled `sha3-keccak` feature in alloy_primitives + no precompiles - 501M cycles (deserialization took 64M)
//
// TIP: In zkVM use more efficient `create_mainnet_evm_config_from` with a bare-minimum chainspec.
pub fn create_mainnet_evm_config() -> reth_ethereum::evm::EthEvmConfig {
  reth_ethereum::evm::EthEvmConfig::mainnet()
}

/// Creates an Ethereum mainnet EVM config from the given chainspec.
/// Measured SP1 performance:
/// - <1K cycles
pub fn create_mainnet_evm_config_from(
  chainspec: alloc::sync::Arc<reth_chainspec::ChainSpec>,
) -> reth_ethereum::evm::EthEvmConfig {
  reth_ethereum::evm::EthEvmConfig::ethereum(chainspec)
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

// One of four main inputs for zkVM execution
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AncestorHeaders {
  // List of ancestors. SHOULD be sorted by block number descending, otherwise validation will fail.
  // NOTE: First item is the 'highest' block from which we get the pre state root hash.
  pub headers: alloc::vec::Vec<alloy_consensus::Header>,
}

// Required for `.number()` calls on sealed blocks.
use alloy_consensus::BlockHeader;

impl AncestorHeaders {
  /// Prepares one of four zkVM inputs - ancestor headers.
  /// Validated data could be retrieved later in zkVM, using `seal_and_validate`.
  pub fn from_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) -> Self {
    // Get headers from the witness, we reverse them to get the desired order.
    // NOTE: Reth provides `ExecutionWitness` with headers sorted by block number ascending:
    // - https://github.com/paradigmxyz/reth/blob/127595e23079de2c494048d0821ea1f1107eb624/crates/rpc/rpc/src/debug.rs#L665-L677
    let mut headers = witness.headers.clone();
    headers.reverse();

    // Decode headers.
    let headers: alloc::vec::Vec<alloy_consensus::Header> = headers
      .into_iter()
      .map(|header_bytes| {
        <alloy_consensus::Header as alloy_rlp::Decodable>::decode(&mut &header_bytes[..])
          .expect("Failed to decode header; witness should be valid")
      })
      .collect();

    Self { headers }
  }

  /// Validates that headers (current block + ancestors) are connected correctly (parent-child relationship),
  /// seals them (to get hash), and returns a map of block numbers to block hashes.
  pub fn seal_and_validate(
    &self,
    current_block: &CurrentBlock,
  ) -> alloy_primitives::map::HashMap<u64, reth_ethereum::evm::revm::primitives::FixedBytes<32>> {
    let mut block_hashes = alloy_primitives::map::HashMap::default();
    let mut sealed_prev: Option<reth_ethereum::primitives::SealedHeader> = None;

    // Chain current block header with ancestor headers.
    let current_header = &current_block.body.header;
    let headers = core::iter::once(current_header).chain(self.headers.iter());

    for header in headers {
      // TODO: Consider avoiding clone.
      let sealed = reth_ethereum::primitives::SealedHeader::seal_slow(header.clone());
      if let Some(prev) = &sealed_prev {
        if sealed.number() != prev.number() - 1 {
          panic!(
            "InvalidHeaderBlockNumber({}, {})",
            prev.number() - 1,
            sealed.number()
          );
        }
        if sealed.hash() != prev.parent_hash() {
          panic!(
            "InvalidHeaderParentHash({}, {})",
            sealed.hash(),
            prev.parent_hash()
          );
        }
        block_hashes.insert(sealed.number(), sealed.hash());
      }

      sealed_prev = Some(sealed);
    }

    block_hashes
  }
}

/// Second main input for zkVM.
#[serde_with::serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CurrentBlock {
  #[serde_as(
    as = "reth_primitives_traits::serde_bincode_compat::Block<'_, alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>, alloy_consensus::Header>"
  )]
  pub body: alloy_consensus::Block<
    alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>,
    alloy_consensus::Header,
  >,
}

impl CurrentBlock {
  // TODO: By taking `self` we could avoid cloning body.
  pub fn recover_senders(
    &self,
  ) -> reth_primitives_traits::RecoveredBlock<
    alloy_consensus::Block<alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>>,
  > {
    reth_primitives_traits::RecoveredBlock::try_recover(self.body.clone()).unwrap()
  }

  // More efficient block recovery, using hinted signers.
  pub fn recover_with_signers_hint(
    self,
    signers_hint: &SignersHint,
  ) -> reth_primitives_traits::RecoveredBlock<
    alloy_consensus::Block<alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>>,
  > {
    // Verify the transaction signatures and compute senders.
    let mut senders = alloc::vec::Vec::with_capacity(
      reth_primitives_traits::BlockBody::transaction_count(&self.body.body),
    );
    for (i, tx) in self.body.body.transactions().enumerate() {
      let vk = &signers_hint.signers[i];
      let sig = tx.signature();
      let sig = match sig.to_k256() {
        Ok(sig) => sig,
        Err(e) => {
          // NOTE: Zeth just ignored this case witout panicking.
          panic!("invalid signature format for tx {i}: {e}");
        }
      };
      vk.verify_prehash(tx.signature_hash().as_slice(), &sig)
        .expect(&alloc::format!("invalid signature for tx {i}"));

      senders.push(alloy_primitives::Address::from_public_key(vk))
    }

    // Construct recovered block (no header rehashing).
    reth_primitives_traits::RecoveredBlock::new_unhashed(self.body, senders)
  }
}

// Fourth main input for zkVM.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bytecodes {
  pub codes: alloy_primitives::map::B256Map<revm::state::Bytecode>,
}

impl Bytecodes {
  /// Prepares one of four zkVM inputs - bytecodes.
  pub fn from_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) -> Self {
    let codes = Self::build_map(&witness.codes);
    Self { codes }
  }

  pub fn build_map(
    codes_list: &alloc::vec::Vec<alloy_primitives::Bytes>,
  ) -> alloy_primitives::map::B256Map<revm::state::Bytecode> {
    let mut bytecode_by_hash: alloy_primitives::map::B256Map<revm::state::Bytecode> =
      alloy_primitives::map::B256Map::default();
    for encoded in codes_list {
      let bytecode = revm::state::Bytecode::new_raw(encoded.clone());
      let code_hash = bytecode.hash_slow();
      bytecode_by_hash.insert(code_hash, bytecode);
    }
    bytecode_by_hash
  }

  /// Used inside zkVM to validate bytecodes map (correctness of keys).
  pub fn validate(&self) {
    for (hash, bytecode) in &self.codes {
      let computed = bytecode.hash_slow();
      if &computed != hash {
        panic!(
          "Invalid bytecode hash: expected {:?}, got {:?}",
          hash, computed
        );
      }
    }
  }
}

pub fn validate_block_post_execution(
  recovered_block: &reth_primitives_traits::RecoveredBlock<
    alloy_consensus::Block<alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>>,
  >,
  chain_spec: &reth_chainspec::ChainSpec,
  execution_output: &reth_execution_types::BlockExecutionOutput<reth_ethereum_primitives::Receipt>,
) {
  if let Err(e) = reth_ethereum_consensus::validate_block_post_execution(
    recovered_block,
    chain_spec,
    &execution_output.result.receipts,
    &execution_output.result.requests,
  ) {
    panic!(
      "Error::Validation(\"Block validation failed after execution: {}\")",
      e
    );
  }
}

pub fn get_hashed_post_state(
  output: &reth_execution_types::BlockExecutionOutput<reth_ethereum_primitives::Receipt>,
) -> reth_trie_common::HashedPostState {
  // Convert the post state from the output to a hashed post state.
  // This is used for further validation or execution.
  reth_trie_common::HashedPostState::from_bundle_state::<reth_trie_common::KeccakKeyHasher>(
    &output.state.state,
  )
}

// Idea from Zeth, to pre-recover signer from the block, then validate them inside zkVM.
// NOTE: We could use `VerifyingKey`'s `.to_sec1_bytes()` and `.from_sec1_bytes()` to make serialization more raw and efficient.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignersHint {
  pub signers: alloc::vec::Vec<alloy_signer::k256::ecdsa::VerifyingKey>,
}

impl SignersHint {
  pub fn from_block(
    block: &alloy_consensus::Block<
      alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>,
      alloy_consensus::Header,
    >,
  ) -> Self {
    let signers = block
      .body
      .transactions()
      .map(|tx| {
        tx.signature()
          .recover_from_prehash(&tx.signature_hash())
          .unwrap()
      })
      .collect();
    Self { signers }
  }
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_create_mainnet_evm_config() {
    let config = super::create_mainnet_evm_config();
    assert_eq!(
      reth_ethereum::chainspec::EthChainSpec::chain_id(&config.chain_spec()),
      1
    );
  }
}
