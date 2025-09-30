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

    // Decode RLP using PayloadView for simple pattern matching.
    // Based on https://github.com/risc0/risc0-ethereum/blob/94dddfe18db9977d7f25fc71d8acc80ba1552aab/crates/trie/src/mpt/rlp.rs#L235-L284.
    let mut buf = &encoded[..];
    match alloy_rlp::Header::decode_raw(&mut buf) {
      Ok(alloy_rlp::PayloadView::String(payload)) => match payload.len() {
        0 => output.push_str("      MPT: Null\n"),
        32 => output.push_str("      MPT: Digest\n"),
        _ => output.push_str("      MPT: Unknown node type\n"),
      },
      Ok(alloy_rlp::PayloadView::List(items)) => match items.len() {
        17 => {
          output.push_str("      MPT: Branch\n");
        }
        2 => {
          // 2-element list: could be Leaf or Extension
          // Use hex-prefix encoding to distinguish:
          //   High nibble = 0b00LO where L=leaf flag, O=odd flag
          let mut path_buf = items[0];
          let Ok(path) = alloy_primitives::Bytes::decode(&mut path_buf) else {
            output.push_str("      MPT: Unknown node type\n");
            continue;
          };

          // Extension node (leaf flag not set in second bit of high nibble)
          if path.is_empty() || (path[0] & 0x20) == 0 {
            output.push_str("      MPT: Extension\n");
            continue;
          }

          // It's a leaf node (0x20 checks the second bit of the high nibble).
          // Try decoding common value types: Account (state trie) or U256 (storage trie).
          let mut val_buf = items[1];
          let Ok(val) = alloy_primitives::Bytes::decode(&mut val_buf) else {
            output.push_str("      MPT: Leaf (Failed to decode value)\n");
            continue;
          };

          // Try to decode as Account first
          let mut account_buf = &val[..];
          if let Ok(account) = <reth_trie::TrieAccount as alloy_rlp::Decodable>::decode(&mut account_buf) {
            output.push_str("      MPT: Leaf (Account):\n");
            output.push_str(&format!("            nonce: {}\n", account.nonce));
            output.push_str(&format!("            balance: {}\n", account.balance));
            output.push_str(&format!("            code_hash: {:#x}\n", account.code_hash));
            output.push_str(&format!("            storage_root: {:#x}\n", account.storage_root));
            continue;
          }

          // Try to decode as U256 (storage value)
          let mut u256_buf = &val[..];
          if let Ok(val) = alloy_primitives::U256::decode(&mut u256_buf) {
            output.push_str(&format!("      MPT: Leaf (Storage Value): {}\n", val));
            continue;
          }

          // Leaf but unknown value type
          output.push_str("      MPT: Leaf (Unknown value)\n");
        }
        _ => {
          output.push_str(&format!("      MPT: List({})\n", items.len()));
        }
      },
      Err(_) => {
        output.push_str("      MPT: Unknown node type\n");
      }
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
