/// TrackingTrie - wraps a StatelessTrie implementation to track accessed addresses
///
/// This wrapper intercepts all account() and storage() calls to record which
/// addresses are actually accessed during block execution.
use alloy_primitives::{Address, B256, U256, map::B256Map};
use alloy_rpc_types_debug::ExecutionWitness;
use reth_errors::ProviderError;
use reth_revm::state::Bytecode;
use reth_stateless::{StatelessTrie, validation::StatelessValidationError};
use reth_trie_common::HashedPostState;
use reth_trie_common::TrieAccount;
use std::cell::RefCell;
use std::collections::HashSet;

/// Shared state for tracking accessed addresses
#[derive(Debug, Default)]
pub struct AccessTracker {
  /// Addresses that had their account data read
  accessed_accounts: RefCell<HashSet<Address>>,
  /// Storage slots that were accessed: (address, slot)
  accessed_storage: RefCell<HashSet<(Address, U256)>>,
}

impl AccessTracker {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn track_account(&self, address: Address) {
    self.accessed_accounts.borrow_mut().insert(address);
  }

  pub fn track_storage(&self, address: Address, slot: U256) {
    self.accessed_storage.borrow_mut().insert((address, slot));
  }

  pub fn get_accessed_accounts(&self) -> Vec<Address> {
    self.accessed_accounts.borrow().iter().copied().collect()
  }

  pub fn get_accessed_storage(&self) -> Vec<(Address, U256)> {
    self.accessed_storage.borrow().iter().copied().collect()
  }
}

/// Wrapper around a StatelessTrie that tracks all accesses
#[derive(Debug)]
pub struct TrackingTrie<T: StatelessTrie> {
  inner: T,
  tracker: AccessTracker,
}

impl<T: StatelessTrie> TrackingTrie<T> {
  pub fn new(inner: T, tracker: AccessTracker) -> Self {
    Self { inner, tracker }
  }

  pub fn tracker(&self) -> &AccessTracker {
    &self.tracker
  }

  pub fn into_inner(self) -> T {
    self.inner
  }
}

impl<T: StatelessTrie> StatelessTrie for TrackingTrie<T> {
  fn new(
    witness: &ExecutionWitness,
    pre_state_root: B256,
  ) -> Result<(Self, B256Map<Bytecode>), StatelessValidationError>
  where
    Self: Sized,
  {
    let (inner, bytecode) = T::new(witness, pre_state_root)?;
    let tracker = AccessTracker::new();
    Ok((Self { inner, tracker }, bytecode))
  }

  fn account(&self, address: Address) -> Result<Option<TrieAccount>, ProviderError> {
    // Track this access!
    self.tracker.track_account(address);

    // Forward to inner trie
    self.inner.account(address)
  }

  fn storage(&self, address: Address, slot: U256) -> Result<U256, ProviderError> {
    // Track this access!
    self.tracker.track_storage(address, slot);

    // Forward to inner trie
    self.inner.storage(address, slot)
  }

  fn calculate_state_root(
    &mut self,
    state: HashedPostState,
  ) -> Result<B256, StatelessValidationError> {
    self.inner.calculate_state_root(state)
  }
}
