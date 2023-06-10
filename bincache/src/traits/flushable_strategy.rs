use async_trait::async_trait;

use super::{CacheKey, CacheStrategy};
use crate::Result;

/// A cache strategy that can flush its data to a non-volatile storage.
#[async_trait]
pub trait FlushableStrategy: CacheStrategy {
    async fn flush<K>(
        &mut self,
        key: &K,
        entry: &Self::CacheEntry,
    ) -> Result<Option<Self::CacheEntry>>
    where
        K: CacheKey + Sync + Send;
}
