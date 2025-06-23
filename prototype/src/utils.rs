use alloy_rlp::Decodable;

/// Debug-print the contents of an ExecutionWitness
pub fn print_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) {
  println!("{}", format_execution_witness(witness));
}

/// Format the contents of an ExecutionWitness as a String
pub fn format_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) -> String {
  let mut output = String::new();

  output.push_str("\n====== 📦 ExecutionWitness Debug Start ======\n\n");

  // ▶️ STATE NODES
  output.push_str(&format!(
    "▶️ {} trie nodes in `state`:\n",
    witness.state.len()
  ));
  for (i, encoded) in witness.state.iter().enumerate() {
    let hash = alloy_primitives::keccak256(encoded);
    output.push_str(&format!("  [{}] node hash: {:#x}\n", i, hash));

    match reth_proofs_core::mpt::MptNode::decode(encoded) {
      Ok(node) => match node.as_data() {
        reth_proofs_core::mpt::MptNodeData::Null => output.push_str("      MPT: Null\n"),
        reth_proofs_core::mpt::MptNodeData::Branch(_) => output.push_str("      MPT: Branch\n"),
        reth_proofs_core::mpt::MptNodeData::Leaf(_, val) => {
          if let Ok(account) =
            <reth_trie::TrieAccount as alloy_rlp::Decodable>::decode(&mut &val[..])
          {
            output.push_str("      MPT: Leaf (Account):\n");
            output.push_str(&format!("            nonce: {}\n", account.nonce));
            output.push_str(&format!("            balance: {}\n", account.balance));
            output.push_str(&format!(
              "            code_hash: {:#x}\n",
              account.code_hash
            ));
            output.push_str(&format!(
              "            storage_root: {:#x}\n",
              account.storage_root
            ));
          } else if let Ok(val) = alloy_primitives::U256::decode(&mut &val[..]) {
            output.push_str(&format!("      MPT: Leaf (Storage Value): {}\n", val));
          } else {
            output.push_str("      MPT: Leaf (Unknown RLP value)\n");
          }
        }
        reth_proofs_core::mpt::MptNodeData::Extension(_, _) => {
          output.push_str("      MPT: Extension\n")
        }
        reth_proofs_core::mpt::MptNodeData::Digest(d) => {
          output.push_str(&format!("      MPT: Digest: {:#x}\n", d))
        }
      },
      Err(e) => output.push_str(&format!("      ❌ Failed to decode MPT node: {:?}\n", e)),
    }
  }

  // ▶️ KEYS
  output.push_str(&format!(
    "\n▶️ {} keys in `keys` (address/slot preimages):\n",
    witness.keys.len()
  ));
  for (i, key) in witness.keys.iter().enumerate() {
    match key.0.len() {
      20 => output.push_str(&format!(
        "  [{}] Address preimage: {}\n",
        i,
        alloy_primitives::hex::encode(&key.0)
      )),
      32 => output.push_str(&format!(
        "  [{}] Slot hash (keccak256): {}\n",
        i,
        alloy_primitives::hex::encode(&key.0)
      )),
      64 => {
        let addr_hash = &key.0[..32];
        let slot_hash = &key.0[32..];
        output.push_str(&format!(
          "  [{}] Storage access — addr_hash: {}, slot_hash: {}\n",
          i,
          alloy_primitives::hex::encode(addr_hash),
          alloy_primitives::hex::encode(slot_hash)
        ));
      }
      len => output.push_str(&format!(
        "  [{}] Unknown key format ({} bytes): {}\n",
        i,
        len,
        alloy_primitives::hex::encode(&key.0)
      )),
    }
  }

  // ▶️ CODES
  output.push_str(&format!(
    "\n▶️ {} codes in `codes` (contract bytecode):\n",
    witness.codes.len()
  ));
  for (i, code) in witness.codes.iter().enumerate() {
    let hash = alloy_primitives::keccak256(code);
    output.push_str(&format!(
      "  [{}] Code hash: {:#x}, len: {} bytes\n",
      i,
      hash,
      code.len()
    ));
  }

  // ▶️ HEADERS
  output.push_str(&format!(
    "\n▶️ {} headers in `headers` (block ancestors):\n",
    witness.headers.len()
  ));
  for (i, encoded) in witness.headers.iter().enumerate() {
    match <alloy_consensus::Header as alloy_rlp::Decodable>::decode(&mut &encoded[..]) {
      Ok(header) => {
        let hash = alloy_primitives::keccak256(alloy_rlp::encode(&header));
        output.push_str(&format!(
          "  [{}] Block #{:<6} — hash: {:#x}, parent: {:#x}\n",
          i, header.number, hash, header.parent_hash
        ));
      }
      Err(e) => {
        output.push_str(&format!("  [{}] ❌ Failed to decode header: {}\n", i, e));
      }
    }
  }

  output.push_str("\n====== ✅ ExecutionWitness Debug End ========");

  output
}
