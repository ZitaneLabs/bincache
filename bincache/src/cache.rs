use crate::{
    traits::{CacheKey, CacheStrategy},
    Result,
};

use std::{borrow::Cow, collections::HashMap, hash::Hash};

/// Binary cache.
#[derive(Debug)]
pub struct Cache<K, S>
where
    K: CacheKey + Eq + Hash,
    S: CacheStrategy,
{
    data: HashMap<K, S::CacheEntry>,
    strategy: S,
}

impl<K, S> Cache<K, S>
where
    K: CacheKey + Eq + Hash,
    S: CacheStrategy,
{
    /// Create a new [Cache].
    pub fn new(strategy: S) -> Cache<K, S> {
        Cache {
            data: HashMap::new(),
            strategy,
        }
    }

    /// Put an entry into the cache.
    pub fn put(&mut self, key: K, value: Vec<u8>) -> Result<()> {
        let entry = self.strategy.put(&key, value)?;
        self.data.insert(key, entry);
        Ok(())
    }

    /// Get an entry from the cache.
    pub fn get(&mut self, key: K) -> Result<Cow<'_, [u8]>> {
        let entry = self.data.get(&key).ok_or(crate::Error::KeyNotFound)?;
        self.strategy.get(entry)
    }

    /// Take an entry from the cache, removing it.
    pub fn take(&mut self, key: K) -> Result<Vec<u8>> {
        let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
        self.strategy.take(entry)
    }

    /// Delete an entry from the cache.
    pub fn delete(&mut self, key: K) -> Result<()> {
        let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
        self.strategy.delete(entry)
    }
}
