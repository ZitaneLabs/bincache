use async_trait::async_trait;

use super::CacheStrategy;
use crate::Result;

/// A cache strategy that can recover its data from a non-volatile storage.
#[async_trait]
pub trait RecoverableStrategy: CacheStrategy {
    /// Discover persisted entries and rebuild strategy accounting.
    ///
    /// Use `recover_key` to parse stored key strings; `None` rejects a key.
    /// The default returns no entries. Called through [`crate::Cache::recover`],
    /// normally once on a fresh strategy. Built-in disk/hybrid recovery ignores
    /// limits, does not verify payload integrity, and leaves entries on disk.
    ///
    /// # Errors
    /// Return unrecoverable storage errors. The caller cannot roll back partial
    /// strategy accounting or filesystem changes.
    async fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        _ = recover_key;
        Ok(vec![])
    }
}
