use async_trait::async_trait;
use std::borrow::Cow;

use crate::{CacheCapacity, Result};

use super::CacheKey;

/// A cache strategy.
#[async_trait]
pub trait CacheStrategy {
    /// This type is opaque to the cache.
    /// It is used to store information about each cached data entry.
    type CacheEntry;

    /// Setup the cache.
    async fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    /// Put a value into the cache.
    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send;

    /// Replace an existing entry without temporarily counting both values.
    ///
    /// On success, release the old resources and account only for the new value.
    /// If this operation fails, `entry` and strategy accounting must remain
    /// valid and unchanged. Capacity checks must credit the old entry's usage.
    ///
    /// This is required for custom strategies: `put` followed by `delete` is
    /// not a safe general fallback when storage locations or limits overlap.
    async fn replace<'a, K, V>(
        &mut self,
        key: &K,
        entry: &mut Self::CacheEntry,
        value: V,
    ) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send;

    /// Get a value from the cache.
    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>>;

    /// Take a value from the cache, removing it.
    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>>;

    /// Delete a value from the cache.
    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()>;

    /// Get cache capacity. Returns None if no limit was set.
    fn get_cache_capacity(&self) -> Option<CacheCapacity>;
}
