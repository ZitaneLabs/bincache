use std::borrow::Cow;

use crate::Result;

use super::CacheKey;

/// A cache strategy.
pub trait CacheStrategy {
    /// This type is opaque to the cache.
    /// It is used to store information about each cached data entry.
    type CacheEntry;

    /// Put a value into the cache.
    fn put(&mut self, key: &impl CacheKey, value: Vec<u8>) -> Result<Self::CacheEntry>;

    /// Get a value from the cache.
    fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>>;

    /// Take a value from the cache, removing it.
    fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>>;

    /// Delete a value from the cache.
    fn delete(&mut self, entry: Self::CacheEntry) -> Result<()>;
}