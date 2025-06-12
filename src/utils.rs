use alloy_rlp::Decodable;

/// Debug-print the contents of an ExecutionWitness
pub fn print_execution_witness(witness: &alloy_rpc_types_debug::ExecutionWitness) {
  println!("\n====== 📦 ExecutionWitness Debug Start ======\n");

  // ▶️ STATE NODES
  println!("▶️ {} trie nodes in `state`:", witness.state.len());
  for (i, encoded) in witness.state.iter().enumerate() {
    let hash = alloy_primitives::keccak256(encoded);
    println!("  [{}] node hash: {:#x}", i, hash);

    match crate::mpt::MptNode::decode(encoded) {
      Ok(node) => match node.as_data() {
        crate::mpt::MptNodeData::Null => println!("      MPT: Null"),
        crate::mpt::MptNodeData::Branch(_) => println!("      MPT: Branch"),
        crate::mpt::MptNodeData::Leaf(_, val) => {
          if let Ok(account) =
            <reth_trie::TrieAccount as alloy_rlp::Decodable>::decode(&mut &val[..])
          {
            println!("      MPT: Leaf (Account):");
            println!("            nonce: {}", account.nonce);
            println!("            balance: {}", account.balance);
            println!("            code_hash: {:#x}", account.code_hash);
            println!("            storage_root: {:#x}", account.storage_root);
          } else if let Ok(val) = alloy_primitives::U256::decode(&mut &val[..]) {
            println!("      MPT: Leaf (Storage Value): {}", val);
          } else {
            println!("      MPT: Leaf (Unknown RLP value)");
          }
        }
        crate::mpt::MptNodeData::Extension(_, _) => println!("      MPT: Extension"),
        crate::mpt::MptNodeData::Digest(d) => println!("      MPT: Digest: {:#x}", d),
      },
      Err(e) => println!("      ❌ Failed to decode MPT node: {e}"),
    }
  }

  // ▶️ KEYS
  println!(
    "\n▶️ {} keys in `keys` (address/slot preimages):",
    witness.keys.len()
  );
  for (i, key) in witness.keys.iter().enumerate() {
    match key.0.len() {
      20 => println!(
        "  [{}] Address preimage: {}",
        i,
        alloy_primitives::hex::encode(&key.0)
      ),
      32 => println!(
        "  [{}] Slot hash (keccak256): {}",
        i,
        alloy_primitives::hex::encode(&key.0)
      ),
      64 => {
        let addr_hash = &key.0[..32];
        let slot_hash = &key.0[32..];
        println!(
          "  [{}] Storage access — addr_hash: {}, slot_hash: {}",
          i,
          alloy_primitives::hex::encode(addr_hash),
          alloy_primitives::hex::encode(slot_hash)
        );
      }
      len => println!(
        "  [{}] Unknown key format ({} bytes): {}",
        i,
        len,
        alloy_primitives::hex::encode(&key.0)
      ),
    }
  }

  // ▶️ CODES
  println!(
    "\n▶️ {} codes in `codes` (contract bytecode):",
    witness.codes.len()
  );
  for (i, code) in witness.codes.iter().enumerate() {
    let hash = alloy_primitives::keccak256(code);
    println!(
      "  [{}] Code hash: {:#x}, len: {} bytes",
      i,
      hash,
      code.len()
    );
  }

  // ▶️ HEADERS
  println!(
    "\n▶️ {} headers in `headers` (block ancestors):",
    witness.headers.len()
  );
  for (i, encoded) in witness.headers.iter().enumerate() {
    match <alloy_consensus::Header as alloy_rlp::Decodable>::decode(&mut &encoded[..]) {
      Ok(header) => {
        let hash = alloy_primitives::keccak256(alloy_rlp::encode(&header));
        println!(
          "  [{}] Block #{:<6} — hash: {:#x}, parent: {:#x}",
          i, header.number, hash, header.parent_hash
        );
      }
      Err(e) => {
        println!("  [{}] ❌ Failed to decode header: {}", i, e);
      }
    }
  }

  println!("\n====== ✅ ExecutionWitness Debug End ========");
}
