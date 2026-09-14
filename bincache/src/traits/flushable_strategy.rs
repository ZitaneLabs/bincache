use async_trait::async_trait;

use super::{CacheKey, CacheStrategy};
use crate::Result;

/// A cache strategy that can flush its data to a non-volatile storage.
#[async_trait]
pub trait FlushableStrategy: CacheStrategy {
    /// Prepare a persistent counterpart to an entry.
    ///
    /// Return `Some(new_entry)` when moved or `None` when already persistent.
    /// The cache subsequently calls `delete` on the old entry and replaces it in
    /// its index. Do not invalidate the old entry during this call. On failure,
    /// the cache does not roll back earlier strategy calls in the flush.
    ///
    /// # Errors
    /// Return storage or capacity errors; partial side effects are possible.
    async fn flush<K>(
        &mut self,
        key: &K,
        entry: &Self::CacheEntry,
    ) -> Result<Option<Self::CacheEntry>>
    where
        K: CacheKey + Sync + Send;
}
