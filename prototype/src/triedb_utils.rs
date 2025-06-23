use std::fmt::Write;

pub fn format_trie(trie: &reth_proofs_core::triedb::TrieDB) -> String {
  let mut out = String::new();

  writeln!(&mut out, "TrieDB {{").unwrap();

  // state_trie
  writeln!(&mut out, "  state_trie:").unwrap();
  print_mpt_node(&mut out, &trie.state_trie, 4);

  // storage_tries
  writeln!(&mut out, "  storage_tries:").unwrap();
  let ordered: std::collections::BTreeMap<_, _> = trie.storage_tries.iter().collect();
  for (k, v) in ordered {
    writeln!(
      out,
      "    {} =>",
      alloy_primitives::hex::encode(k.as_slice())
    )
    .unwrap();
    print_mpt_node(&mut out, v, 6);
  }

  // block_hashes
  writeln!(&mut out, "  block_hashes: {{").unwrap();
  let ordered: std::collections::BTreeMap<_, _> = trie.block_hashes.iter().collect();
  for (k, v) in ordered {
    writeln!(
      &mut out,
      "    {} => {}",
      k,
      alloy_primitives::hex::encode(v.as_slice())
    )
    .unwrap();
  }
  writeln!(&mut out, "  }}").unwrap();

  // bytecode_by_hash (show hash and code length only)
  writeln!(&mut out, "  bytecode_by_hash: [").unwrap();
  let ordered_bc: std::collections::BTreeMap<_, _> = trie.bytecode_by_hash.iter().collect();
  for (h, code) in ordered_bc {
    writeln!(
      &mut out,
      "    {} (len: {}),",
      alloy_primitives::hex::encode(h.as_slice()),
      code.bytes().len()
    )
    .unwrap();
  }
  writeln!(&mut out, "  ]").unwrap();

  writeln!(&mut out, "}}").unwrap();
  out
}

fn print_mpt_node(out: &mut String, node: &reth_proofs_core::mpt::MptNode, indent: usize) {
  let pad = " ".repeat(indent);
  writeln!(out, "{}MptNode:", pad).unwrap();

  match &node.data {
    reth_proofs_core::mpt::MptNodeData::Null => writeln!(out, "{}  Null", pad).unwrap(),

    reth_proofs_core::mpt::MptNodeData::Leaf(k, v) => {
      writeln!(
        out,
        "{}  Leaf: key={}, value={}",
        pad,
        alloy_primitives::hex::encode(k),
        alloy_primitives::hex::encode(v)
      )
      .unwrap();
    }

    reth_proofs_core::mpt::MptNodeData::Extension(k, child) => {
      writeln!(
        out,
        "{}  Extension: key={}",
        pad,
        alloy_primitives::hex::encode(k)
      )
      .unwrap();
      print_mpt_node(out, child, indent + 4);
    }

    reth_proofs_core::mpt::MptNodeData::Digest(h) => {
      writeln!(
        out,
        "{}  Digest: {}",
        pad,
        alloy_primitives::hex::encode(h.as_slice())
      )
      .unwrap();
    }

    reth_proofs_core::mpt::MptNodeData::Branch(children) => {
      writeln!(out, "{}  Branch:", pad).unwrap();
      for (i, child) in children.iter().enumerate() {
        if let Some(c) = child {
          writeln!(out, "{}    [{}]:", pad, i).unwrap();
          print_mpt_node(out, c, indent + 6);
        }
      }
    }
  }

  // Also print cached reference if present
  if let Some(reference) = node.cached_reference.borrow().as_ref() {
    print_node_reference(out, reference, indent + 2);
  }
}

fn print_node_reference(
  out: &mut String,
  reference: &reth_proofs_core::mpt::MptNodeReference,
  indent: usize,
) {
  let pad = " ".repeat(indent);
  match reference {
    reth_proofs_core::mpt::MptNodeReference::Bytes(b) => {
      writeln!(
        out,
        "{}Ref::Bytes({})",
        pad,
        alloy_primitives::hex::encode(b)
      )
      .unwrap();
    }
    reth_proofs_core::mpt::MptNodeReference::Digest(h) => {
      writeln!(
        out,
        "{}Ref::Digest({})",
        pad,
        alloy_primitives::hex::encode(h.as_slice())
      )
      .unwrap();
    }
  }
}
