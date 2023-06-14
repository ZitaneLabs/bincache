use crate::{
    CacheKey, CacheStrategy, CompressionStrategy, FlushableStrategy, RecoverableStrategy, Result,
};

use std::{borrow::Cow, collections::HashMap, hash::Hash};

/// Binary cache.
#[derive(Debug)]
pub struct Cache<K, S, C>
where
    K: CacheKey + Eq + Hash,
    S: CacheStrategy,
    C: CompressionStrategy + Sync + Send,
{
    data: HashMap<K, S::CacheEntry>,
    strategy: S,
    compressor: Option<C>,
}

impl<K, S, C> Cache<K, S, C>
where
    K: CacheKey + Eq + Hash + Sync + Send,
    S: CacheStrategy + Send,
    C: CompressionStrategy + Sync + Send,
{
    /// Create a new [Cache].
    pub async fn new(mut strategy: S, compressor: Option<C>) -> Result<Cache<K, S, C>>
    where
        C: CompressionStrategy + Sync + Send,
    {
        strategy.setup().await?;
        Ok(Cache {
            data: HashMap::new(),
            strategy,
            compressor,
        })
    }

    /// Put an entry into the cache.
    pub async fn put<'a, V>(&mut self, key: K, value: V) -> Result<()>
    where
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value: Cow<'_, [u8]> = self.compressor.compress(value.into()).await?;

        let entry = self.strategy.put(&key, value).await?;
        self.data.insert(key, entry);
        Ok(())
    }

    /// Get an entry from the cache.
    pub async fn get(&self, key: K) -> Result<Cow<'_, [u8]>> {
        let entry = self.data.get(&key).ok_or(crate::Error::KeyNotFound)?;
        let value = self.strategy.get(entry).await?;
        self.compressor.decompress(value).await
    }

    /// Take an entry from the cache, removing it.
    pub async fn take(&mut self, key: K) -> Result<Vec<u8>> {
        let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
        let value = self.strategy.take(entry).await?;
        Ok(self.compressor.decompress(value.into()).await?.into_owned())
    }

    /// Delete an entry from the cache.
    pub async fn delete(&mut self, key: K) -> Result<()> {
        let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
        self.strategy.delete(entry).await
    }

    /// Check if an entry exists.
    pub fn exists(&self, key: K) -> bool {
        self.data.contains_key(&key)
    }

    #[cfg(test)]
    pub(crate) fn strategy(&self) -> &S {
        &self.strategy
    }
}

impl<K, S, C> Cache<K, S, C>
where
    K: CacheKey + Eq + Hash + Send,
    S: RecoverableStrategy + Send,
    C: CompressionStrategy + Sync + Send,
{
    /// Recover the cache from a previous state.
    /// Returns the number of recovered items.
    ///
    /// ## When to recover
    /// Useful after a crash or unplanned restart. It's good practice to call this
    /// method on startup, but it depends on your specific use case.
    ///
    /// ## Disclaimer
    /// This is a best-effort operation, full recovery is not guaranteed.
    pub async fn recover<F>(&mut self, key_from_str: F) -> Result<usize>
    where
        F: Fn(&str) -> Option<K> + Send,
    {
        // Recover cache using the strategy
        let entries = self.strategy.recover(key_from_str).await?;
        let recovered_item_count = entries.len();

        // Insert recovered entries into the cache
        for (key, entry) in entries {
            self.data.insert(key, entry);
        }

        Ok(recovered_item_count)
    }
}

impl<K, S, C> Cache<K, S, C>
where
    K: CacheKey + Eq + Hash + ToOwned<Owned = K> + Sync + Send,
    S: FlushableStrategy,
    C: CompressionStrategy + Sync + Send,
{
    /// Flush entries to an underlying non-volatile storage.
    /// Returns the number of flushed items.
    pub async fn flush(&mut self) -> Result<usize> {
        let mut flushed_item_count = 0;
        let mut keys_to_remove = Vec::<K>::new();
        let mut entries_to_insert = Vec::new();

        // Flush all entries using the strategy
        for (key, entry) in self.data.iter() {
            let Some(new_entry) = self.strategy.flush(key, entry).await? else {
                continue;
            };
            keys_to_remove.push(key.to_owned());
            entries_to_insert.push((key.to_owned(), new_entry));
            flushed_item_count += 1;
        }

        // Remove flushed entries from the cache
        for key in keys_to_remove {
            let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
            self.strategy.delete(entry).await?;
        }

        // Insert moved entries into the cache
        for (key, entry) in entries_to_insert {
            self.data.insert(key, entry);
        }

        Ok(flushed_item_count)
    }
}
