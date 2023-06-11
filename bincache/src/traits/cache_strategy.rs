use async_trait::async_trait;
use std::borrow::Cow;

use crate::Result;

use super::CacheKey;

/// A cache strategy.
#[async_trait]
pub trait CacheStrategy {
    /// This type is opaque to the cache.
    /// It is used to store information about each cached data entry.
    type CacheEntry;

    /// Put a value into the cache.
    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send;

    /// Get a value from the cache.
    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>>;

    /// Take a value from the cache, removing it.
    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>>;

    /// Delete a value from the cache.
    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()>;
}
