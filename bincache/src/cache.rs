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

    /// Recover the cache from a previous state.
    /// Returns the number of recovered items.
    ///
    /// ## When to recover
    /// Useful after a crash or unplanned restart. It's good practice to call this
    /// method on startup, but it depends on your specific use case.
    ///
    /// ## Disclaimer
    /// This is a best-effort operation, full recovery is not guaranteed.
    /// For memory-based caches, no recovery is possible.
    pub fn recover<F>(&mut self, key_from_str: F) -> Result<usize>
    where
        F: Fn(&str) -> Option<K>,
    {
        // Recover cache using the strategy
        let entries = self.strategy.recover(key_from_str)?;
        let recovered_item_count = entries.len();

        // Insert recovered entries into the cache
        for (key, entry) in entries {
            self.data.insert(key, entry);
        }

        Ok(recovered_item_count)
    }

    /// Put an entry into the cache.
    pub fn put<'a>(&mut self, key: K, value: impl Into<Cow<'a, [u8]>>) -> Result<()> {
        let entry = self.strategy.put(&key, value)?;
        self.data.insert(key, entry);
        Ok(())
    }

    /// Get an entry from the cache.
    pub fn get(&self, key: K) -> Result<Cow<'_, [u8]>> {
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

    #[cfg(test)]
    pub(crate) fn strategy(&self) -> &S {
        &self.strategy
    }
}
