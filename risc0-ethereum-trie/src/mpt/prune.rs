use std::iter::Peekable;
use std::slice::Iter;

/// A cursor for traversing a sorted list of trie keys, used during pruning.
///
/// The cursor maintains its position in the key list and provides methods to seek
/// and check for keys matching specific path prefixes. Since trie traversal is
/// lexicographic, we can advance through keys linearly without backtracking.
#[derive(Clone)]
pub struct SortedKeysCursor<'a> {
    it: Peekable<Iter<'a, alloy_trie::Nibbles>>,
}

impl<'a> SortedKeysCursor<'a> {
    /// Creates a new cursor over the given set of sorted keys.
    /// CAUTION: Panics if keys are NOT sorted lexicographically.
    pub fn new(keys: &'a [alloy_trie::Nibbles]) -> Self {
        // Make sure keys are sorted and without duplicates.
        assert!(keys.windows(2).all(|w| w[0].as_slice() < w[1].as_slice()));

        Self { it: keys.iter().peekable() }
    }

    /// Peeks at the current key without consuming it.
    pub fn peek(&mut self) -> Option<&'a alloy_trie::Nibbles> {
        self.it.peek().copied()
    }

    /// Advances the cursor until the current key is >= `lookup_key` (lexicographically).
    ///
    /// Consumes all keys that are strictly less than `lookup_key`.
    pub fn seek(&mut self, lookup_key: &[u8]) {
        while let Some(key) = self.peek() {
            if key.as_slice() < lookup_key {
                self.it.next();
            } else {
                break;
            }
        }
    }

    /// Seeks to the first key >= `prefix`, then returns it if it starts with `prefix`.
    ///
    /// Does not consume the matched key, allowing multiple checks at the same position.
    /// Returns `None` if no remaining keys start with the given prefix.
    pub fn peek_with_prefix(&mut self, prefix: &[u8]) -> Option<&'a alloy_trie::Nibbles> {
        self.seek(prefix);
        self.peek().filter(|t| t.as_slice().starts_with(prefix))
    }
}
