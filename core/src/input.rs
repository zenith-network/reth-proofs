pub use reth_trie_sp1_zkvm::SP1ZkvmTrie as EthereumState;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ZkvmInput {
  pub ancestor_headers: crate::AncestorHeaders,
  pub current_block: crate::CurrentBlock,
  pub ethereum_state: EthereumState,
  pub bytecodes: crate::Bytecodes,
}

impl ZkvmInput {
  /// Prepares zkVM input, based on the full block and its execution witness (reth RPC data).
  /// NOTE: Witness `keys` are not used.
  pub fn from_offline_rpc_data(
    block: alloy_consensus::Block<
      alloy_consensus::EthereumTxEnvelope<alloy_consensus::TxEip4844>,
      alloy_consensus::Header,
    >,
    witness: &alloy_rpc_types_debug::ExecutionWitness,
  ) -> Self {
    // Prepare ancestor headers and extract the pre-state root.
    let ancestor_headers = crate::AncestorHeaders::from_execution_witness(witness);
    let pre_state_root = ancestor_headers.headers.first().unwrap().state_root;

    // Prepare current block.
    let current_block = crate::CurrentBlock { body: block };

    // Prepare Ethereum state.
    let ethereum_state = EthereumState::from_execution_witness(witness, pre_state_root);

    // Prepare bytecodes.
    let bytecodes = crate::Bytecodes::from_execution_witness(&witness);

    Self {
      ancestor_headers,
      current_block,
      ethereum_state,
      bytecodes,
    }
  }
}
