// Code based on https://github.com/risc0/zeth/blob/1ecdfa2325af161b529a4ad4cb2b6ce949679a28/crates/core/src/mpt.rs.
use alloy_primitives::{b256, B256};
use alloy_rlp::{Decodable, Encodable};
use risc0_ethereum_trie::CachedTrie;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// Root hash of an empty Merkle Patricia trie, i.e. `keccak256(RLP(""))`.
pub const EMPTY_ROOT: B256 =
  b256!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421");

#[derive(
  Debug,
  Clone,
  Eq,
  PartialEq,
  Deserialize,
  Serialize,
  // rkyv::Archive,
  // rkyv::Serialize,
  // rkyv::Deserialize,
)]
pub struct MptNode<T: Decodable + Encodable> {
  inner: CachedTrie,
  phantom_data: PhantomData<T>,
}

impl<T: Decodable + Encodable> Default for MptNode<T> {
  fn default() -> Self {
    Self {
      inner: CachedTrie::default(),
      phantom_data: PhantomData,
    }
  }
}

impl<T: Decodable + Encodable> MptNode<T> {
  pub fn get_rlp(&self, key: impl AsRef<[u8]>) -> alloy_rlp::Result<Option<T>> {
    match self.inner.get(key) {
      None => Ok(None),
      Some(mut bytes) => Ok(Some(T::decode(&mut bytes)?)),
    }
  }

  pub fn insert_rlp<K, V>(&mut self, key: K, value: V)
  where
    K: AsRef<[u8]>,
    V: Borrow<T>,
  {
    self.inner.insert(key, alloy_rlp::encode(value.borrow()));
  }

  #[inline]
  pub fn from_digest(digest: B256) -> Self {
    if digest == B256::ZERO {
      Self::default()
    } else {
      Self {
        inner: CachedTrie::from_digest(digest),
        phantom_data: PhantomData,
      }
    }
  }

  #[inline]
  pub fn from_rlp<N: AsRef<[u8]>>(nodes: impl IntoIterator<Item = N>) -> alloy_rlp::Result<Self> {
    Ok(Self {
      inner: CachedTrie::from_rlp(nodes)?,
      phantom_data: PhantomData,
    })
  }
}

impl<T: Decodable + Encodable> Deref for MptNode<T> {
  type Target = CachedTrie;

  #[inline]
  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

impl<T: Decodable + Encodable> DerefMut for MptNode<T> {
  #[inline]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.inner
  }
}
