use std::fmt::Write;

pub fn format_trie(trie: &reth_proofs_core::triedb::TrieDB) -> String {
  let mut out = String::new();

  panic!("Not supported for new Trie!");
  /*writeln!(&mut out, "TrieDB {{").unwrap();

  // state
  // NOTE: RSP's MPT debug printing is submitted upstream - https://github.com/succinctlabs/rsp/pull/156.
  writeln!(&mut out, "  state:").unwrap();
  writeln!(&mut out, "{:#?}", trie.state).unwrap();

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

  writeln!(&mut out, "}}").unwrap();*/
  out
}
