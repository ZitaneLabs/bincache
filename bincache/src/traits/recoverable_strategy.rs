use async_trait::async_trait;

use super::CacheStrategy;
use crate::Result;

/// A cache strategy that can recover its data from a non-volatile storage.
#[async_trait]
pub trait RecoverableStrategy: CacheStrategy {
    /// Attempt to recover the cache from a crash.
    async fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        _ = recover_key;
        Ok(vec![])
    }
}
