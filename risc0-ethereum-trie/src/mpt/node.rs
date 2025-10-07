// Copyright 2025 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::{
    children::{Children, Entry},
    memoize::Memoization,
    nibbles::NibbleSlice,
};
use alloy_primitives::{Bytes, B256};
use alloy_trie::Nibbles;
use std::mem;

pub(super) type Child<M> = Box<Node<M>>;

#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(bound(serialize = "", deserialize = "M: Default"))
)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize),
    rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext))),
    rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source)),
    rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))
)]
pub(super) enum Node<M> {
    #[default]
    Null,
    Leaf(
        #[cfg_attr(feature = "rkyv", rkyv(with = super::rkyv::NibblesDef))] Nibbles,
        #[cfg_attr(feature = "rkyv", rkyv(with = super::rkyv::BytesDef))] Bytes,
        #[cfg_attr(feature = "serde", serde(skip))]
        #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
        M,
    ),
    Extension(
        #[cfg_attr(feature = "rkyv", rkyv(with = super::rkyv::NibblesDef))] Nibbles,
        #[cfg_attr(feature = "rkyv", rkyv(omit_bounds))] Child<M>,
        #[cfg_attr(feature = "serde", serde(skip))]
        #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
        M,
    ),
    Branch(
        Children<M>,
        #[cfg_attr(feature = "serde", serde(skip))]
        #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
        M,
    ),
    Digest(#[cfg_attr(feature = "rkyv", rkyv(with = super::rkyv::B256Def))] B256),
}

impl<M> PartialEq for Node<M> {
    /// Equality between nodes ignores the cache.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Node::Null, Node::Null) => true,
            (Node::Leaf(n1, b1, _), Node::Leaf(n2, b2, _)) => n1 == n2 && b1 == b2,
            (Node::Extension(n1, c1, _), Node::Extension(n2, c2, _)) => n1 == n2 && c1 == c2,
            (Node::Branch(c1, _), Node::Branch(c2, _)) => c1 == c2,
            (Node::Digest(d1), Node::Digest(d2)) => d1 == d2,
            _ => false, // different variants are not equal
        }
    }
}

impl<M> Eq for Node<M> {}

impl<M: Memoization> Node<M> {
    /// Retrieves the value associated with a given key.
    pub(super) fn get(&self, key: NibbleSlice<'_>) -> Option<&Bytes> {
        match self {
            Node::Null => None,
            Node::Leaf(prefix, value, _) if prefix == key.as_slice() => Some(value),
            Node::Leaf(..) => None,
            Node::Extension(prefix, child, _) => {
                key.strip_prefix(prefix).and_then(|tail| child.get(tail))
            }
            Node::Branch(children, _) => match key.split_first() {
                Some((nib, tail)) => {
                    // SAFETY: `key` is a `NibbleSlice` and thus only contains values < 0xf
                    let child = unsafe { children.get_unchecked(nib) };
                    child.and_then(|node| node.get(tail))
                }
                None => None, // branch nodes don't have values in our MPT version
            },
            Node::Digest(_) => panic!("MPT: Unresolved node access (get)"),
        }
    }

    /// Inserts a key-value pair into the trie.
    pub(super) fn insert(&mut self, key: NibbleSlice<'_>, value: Bytes) {
        assert!(!value.is_empty());
        match self {
            Node::Null => {
                *self = Node::Leaf(key.into(), value, M::default());
            }
            Node::Leaf(prefix, leaf_val, cache) => {
                let (common, key_rem, prefix_rem) = key.split_common_prefix(&*prefix);
                if common.len() == prefix.len() && common.len() == key.len() {
                    *leaf_val = value;
                    cache.clear();
                    return;
                } else if common.len() == prefix.len() || common.len() == key.len() {
                    panic!("MPT: Value in branch");
                }

                let mut children = Children::default();
                match prefix_rem.split_first() {
                    Some((nib, tail)) => {
                        children.insert(
                            nib,
                            Node::Leaf(tail.into(), mem::take(leaf_val), M::default()).into(),
                        );
                    }
                    None => unreachable!(), // mid < prefix.len()
                }
                match key_rem.split_first() {
                    Some((nib, tail)) => {
                        children.insert(nib, Node::Leaf(tail.into(), value, M::default()).into())
                    }
                    None => unreachable!(), // mid < key.len()
                };
                let branch = Node::Branch(children, M::default());

                *self = if common.is_empty() {
                    branch
                } else {
                    Node::Extension(common.into(), branch.into(), M::default())
                };
            }
            Node::Extension(prefix, child, cache) => {
                let (common, key_rem, prefix_rem) = key.split_common_prefix(&*prefix);
                if common.len() == prefix.len() {
                    child.insert(key_rem, value);
                    cache.clear();
                    return;
                } else if common.len() == key.len() {
                    panic!("MPT: Value in branch");
                }

                let mut children = Children::default();
                match prefix_rem.as_slice() {
                    [nib] => children.insert(*nib, mem::take(child)),
                    [nib, tail @ ..] => {
                        // SAFETY: `tail` is a slice of `prefix` and thus only contains nibbles
                        let prefix = Nibbles::from_nibbles_unchecked(tail);
                        children.insert(
                            *nib,
                            Node::Extension(prefix, mem::take(child), M::default()).into(),
                        );
                    }
                    _ => unreachable!(), // mid < prefix.len()
                }
                match key_rem.split_first() {
                    Some((nib, tail)) => {
                        children.insert(nib, Node::Leaf(tail.into(), value, M::default()).into())
                    }
                    None => unreachable!(), // mid < key.len()
                };
                let branch = Node::Branch(children, M::default());

                *self = if common.is_empty() {
                    branch
                } else {
                    Node::Extension(common.into(), branch.into(), M::default())
                };
            }
            Node::Branch(children, cache) => match key.split_first() {
                Some((nib, tail)) => match children.entry(nib) {
                    Entry::Occupied(mut entry) => {
                        entry.get_mut().insert(tail, value);
                        cache.clear();
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(Node::Leaf(tail.into(), value, M::default()).into());
                        cache.clear();
                    }
                },
                None => panic!("MPT: Value in branch"),
            },
            Node::Digest(_) => panic!("MPT: Unresolved node access (insert)"),
        }
    }

    /// Removes a key-value pair from the trie.
    pub(super) fn remove(&mut self, key: NibbleSlice<'_>) -> bool {
        match self {
            Node::Null => false,
            Node::Leaf(prefix, ..) if prefix == key.as_slice() => {
                *self = Node::Null;
                true
            }
            Node::Leaf(..) => false,
            Node::Extension(prefix, child, cache) => {
                if !key.strip_prefix(&*prefix).is_some_and(|tail| child.remove(tail)) {
                    return false;
                }
                cache.clear();

                // an extension always points to a branch, if this has changed because of the remove
                match **child {
                    Node::Null => *self = Node::Null,
                    Node::Leaf(ref extension, ref mut value, _) => {
                        prefix.extend_from_slice(extension);
                        *self = Node::Leaf(mem::take(prefix), mem::take(value), M::default())
                    }
                    Node::Extension(ref extension, ref mut child, _) => {
                        prefix.extend_from_slice(extension);
                        *self = Node::Extension(mem::take(prefix), mem::take(child), M::default())
                    }
                    Node::Branch(..) => {}
                    Node::Digest(_) => unreachable!(), // child.remove() would have panicked
                }
                true
            }
            Node::Branch(children, cache) => {
                match key.split_first() {
                    Some((nib, tail)) => match children.entry(nib) {
                        Entry::Occupied(mut entry) => {
                            if !entry.get_mut().remove(tail) {
                                return false;
                            }
                        }
                        Entry::Vacant(_) => return false,
                    },
                    None => return false, // branch nodes don't have values in our MPT version
                };
                cache.clear();

                if let Some((nib, only_child)) = children.take_single_child() {
                    match *only_child {
                        // if the only child is a leaf, prepend the corresponding nib to it
                        Node::Leaf(mut extension, value, _) => {
                            // SAFETY: `take_single_child` always returns a nibble
                            extension.as_mut_vec_unchecked().insert(0, nib);
                            *self = Node::Leaf(extension, value, M::default());
                        }
                        // if the only child is an extension, prepend the corresponding nib to it
                        Node::Extension(mut extension, child, ..) => {
                            // SAFETY: `take_single_child` always returns a nibble
                            extension.as_mut_vec_unchecked().insert(0, nib);
                            *self = Node::Extension(extension, child, M::default());
                        }
                        // if the only child is a branch, convert to an extension
                        Node::Branch(..) => {
                            // SAFETY: `take_single_child` always returns a nibble
                            let prefix = Nibbles::from_nibbles_unchecked([nib]);
                            *self = Node::Extension(prefix, only_child, M::default());
                        }
                        Node::Digest(_) => panic!("MPT: Unresolved node access (remove)"),
                        Node::Null => unreachable!(), // children does not contain any Node::Null
                    }
                }
                true
            }
            Node::Digest(_) => panic!("MPT: Unresolved node access (remove2)"),
        }
    }

    /// Returns the number of full nodes in the trie.
    pub(super) fn size(&self) -> usize {
        match self {
            Node::Null | Node::Digest(_) => 0,
            Node::Leaf(..) => 1,
            Node::Extension(_, child, ..) => 1 + child.size(),
            Node::Branch(children, ..) => {
                1 + children.iter().filter_map(Option::as_deref).map(Node::size).sum::<usize>()
            }
        }
    }

    /// Replaces unused parts of trie with their digest representation, leaving
    /// only the minimal nodes needed to reach the specified set of keys.
    ///
    /// # Arguments
    /// * `current_path` - The path from root to the current node.
    /// * `cursor` - Mutable cursor tracking position in the sorted keys list.
    #[cfg(feature = "prune")]
    pub(super) fn prune_to_keys(
        &mut self,
        current_path: &Nibbles,
        keys_cursor: &mut super::prune::SortedKeysCursor<'_>,
    ) {
        // If no target key is reachable from the current path, then we can prune this node.
        if keys_cursor.peek_with_prefix(current_path.as_slice()).is_none() {
            let hash = self.hash();
            *self = Node::Digest(hash);
            return;
        }

        // Recursively prune underlying nodes.
        match self {
            Node::Digest(_) | Node::Null => {
                // Already pruned or empty.
            }
            Node::Leaf(_, _, _) => {
                // Keep leaf, as it was reachable.
            }
            Node::Extension(prefix, child, _) => {
                // Prune underlying child.
                let mut extended_path = current_path.clone();
                extended_path.extend_from_slice_unchecked(prefix);
                child.prune_to_keys(&extended_path, keys_cursor);
            }
            Node::Branch(children, _) => {
                // Prune children 0-15 in lexicographic order.
                for i in 0..16 {
                    if let Entry::Occupied(mut entry) = children.entry(i as u8) {
                        let mut child_path = current_path.clone();
                        child_path.as_mut_vec_unchecked().push(i as u8);
                        entry.get_mut().prune_to_keys(&child_path, keys_cursor);
                    }
                }
            }
        }
    }
}
