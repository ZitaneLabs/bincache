use async_trait::async_trait;

use super::{CacheKey, CacheStrategy};
use crate::Result;

/// A cache strategy that can flush its data to a non-volatile storage.
#[async_trait]
pub trait FlushableStrategy: CacheStrategy {
    /// Flush and update one entry and its accounting in place. Return whether
    /// it changed. The entry must remain usable if the call fails or is canceled.
    async fn flush<K>(&mut self, key: &K, entry: &mut Self::CacheEntry) -> Result<bool>
    where
        K: CacheKey + Sync + Send;
}
