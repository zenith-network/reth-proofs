//#![no_std] <- TODO: Reenable after replacing bincode with rkyv.

use reth_consensus::Consensus;
use reth_consensus::HeaderValidator;

extern crate alloc;

pub fn guest_handler(input_buffer: &[u8]) {
  // 1. Reading input data from stdin.
  let zkvm_input =
    bincode::deserialize::<reth_proofs_core::input::ZkvmInput>(&input_buffer).unwrap();
  let ancestor_headers = zkvm_input.ancestor_headers;
  let current_block = zkvm_input.current_block;
  let ethereum_state = zkvm_input.ethereum_state;
  let bytecodes = zkvm_input.bytecodes;
  let signers_hint = zkvm_input.signers_hint;

  // 2. Creating a mainnet EVM config.
  let chainspec = reth_proofs_core::create_mainnet_chainspec();
  let chainspec_arc = alloc::sync::Arc::new(chainspec);
  let evm_config = reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc.clone());

  // 3. Sealing and validating all headers.
  let (block_hashes, last_sealed) = ancestor_headers.seal_and_validate(&current_block);
  let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;

  // 4. Validating state trie.
  reth_trie_risc0_zkvm::validate_state_trie(&ethereum_state.state_trie, pre_state_root);

  // 5. Validating storage tries.
  reth_trie_risc0_zkvm::validate_storage_tries(
    &ethereum_state.state_trie,
    &ethereum_state.storage_tries,
  )
  .unwrap();

  // 6. Validating bytecode map.
  bytecodes.validate();
  let bytecode_by_hash = bytecodes.codes;

  // 7. Prepare database for EVM execution.
  let mut trie_db = reth_proofs_core::triedb::TrieDB {
    state: ethereum_state,
    bytecode_by_hash,
    block_hashes,
  };
  let db = reth_proofs_core::triedb::wrap_into_database(&trie_db);

  // 8. Create block executor.
  let block_executor =
    reth_ethereum::evm::primitives::execute::BasicBlockExecutor::new(&evm_config, db);

  // 9. Recover block signatures.
  //let recovered_block = current_block.recover_senders(); // <- Fast in SP1, but terribly slow in R0.
  let recovered_block = current_block.recover_with_signers_hint(&signers_hint);

  // 9.5. Extra header validation.
  let consensus = reth_ethereum_consensus::EthBeaconConsensus::new(chainspec_arc.clone());
  consensus
    .validate_header(recovered_block.sealed_header())
    .unwrap();
  consensus
    .validate_header_against_parent(recovered_block.sealed_header(), &last_sealed)
    .unwrap();
  consensus
    .validate_block_pre_execution(&recovered_block)
    .unwrap();

  // 10. Execute block.
  let output =
    reth_ethereum::evm::primitives::execute::Executor::execute(block_executor, &recovered_block)
      .unwrap();

  // 11. Validate block post execution.
  reth_proofs_core::validate_block_post_execution(
    &recovered_block,
    chainspec_arc.as_ref(),
    &output,
  );

  // 12. Get hashed post state.
  let hashed_post_state = reth_proofs_core::get_hashed_post_state(&output);

  // 13. Apply state updates.
  trie_db.state.update(hashed_post_state);

  // 14. Compute new state root and verify.
  let new_state_root = trie_db.state.compute_state_root();
  let expected_root = recovered_block.header().state_root;
  if new_state_root != expected_root {
    panic!(
      "New state root does not match expected root: {:?} != {:?}",
      new_state_root, expected_root
    );
  }
}

pub fn guest_alt_handler(input_buffer: &[u8]) {
  // 1. Reading input data from stdin.
  let zkvm_input =
    bincode::deserialize::<reth_proofs_core::input_alt::ZkvmAltInput>(&input_buffer).unwrap();
  let current_block = zkvm_input.block.body;
  let witness = zkvm_input.witness;

  // 2. Creating a mainnet EVM config.
  let chainspec = reth_proofs_core::create_mainnet_chainspec();
  let chainspec_arc = alloc::sync::Arc::new(chainspec);
  let evm_config = reth_proofs_core::create_mainnet_evm_config_from(chainspec_arc.clone());

  // 3. Validate using `reth-stateless` crate.
  // 3. Signers recovery.
  // NOTE: Based on https://github.com/paradigmxyz/reth/blob/e3b38b2de5be10edf7c17e4e895ad1bd0a9b02f2/testing/ef-tests/src/cases/blockchain_test.rs#L427.
  let public_keys = current_block
    .body
    .transactions()
    .into_iter()
    .enumerate()
    .map(|(i, tx)| {
      tx.signature()
        .recover_from_prehash(&tx.signature_hash())
        .map(|keys| {
          reth_stateless::UncompressedPublicKey(
            keys.to_encoded_point(false).as_bytes().try_into().unwrap(),
          )
        })
        .map_err(|e| format!("failed to recover signature for tx #{i}: {e}"))
    })
    .collect::<Result<Vec<reth_stateless::UncompressedPublicKey>, _>>();
  let public_keys = public_keys.unwrap();

  // 4. Validate using `reth-stateless` crate.
  // reth_stateless::stateless_validation_with_trie::<reth_stateless::trie::StatelessSparseTrie, _, _>(current_block, witness, chainspec_arc, evm_config).unwrap();
  reth_stateless::stateless_validation_with_trie::<reth_trie_risc0_zkvm::Risc0ZkvmTrie, _, _>(
    current_block,
    public_keys,
    witness,
    chainspec_arc,
    evm_config,
  )
  .unwrap();
}
