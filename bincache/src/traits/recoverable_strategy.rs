use super::CacheStrategy;
use crate::Result;

/// A cache strategy that can recover its data from a non-volatile storage.
pub trait RecoverableStrategy: CacheStrategy {
    /// Attempt to recover the cache from a crash.
    fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        F: Fn(&str) -> Option<K>,
    {
        _ = recover_key;
        Ok(vec![])
    }
}
