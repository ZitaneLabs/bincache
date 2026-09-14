use crate::{
    CacheCapacity, CacheKey, CacheStrategy, CompressionStrategy, FlushableStrategy,
    RecoverableStrategy, Result,
};

use std::{borrow::Cow, collections::HashMap, hash::Hash};

/// An indexed binary cache with pluggable storage and optional compression.
///
/// The cache owns its keys and entry index; constructing it does not recover files.
/// Mutations require exclusive access. `Send`/`Sync` depend on the generic types,
/// including `S::CacheEntry`; there is no internal synchronization. Use an async
/// lock around a shared instance and retain the guard through each operation.
/// Use only one live instance per disk directory.
///
/// See the [crate documentation](crate) for capacity, recovery, persistence,
/// cancellation, and partial-failure limitations. Built-in disk formats are not
/// a stable interchange format.
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
    /// Initialize storage and start with an empty index.
    ///
    /// `None` disables compression; [`crate::NO_COMPRESSION`] supplies its type.
    /// Disk/hybrid setup creates the directory but does not scan existing files.
    /// Use [`Cache::recover`] once on startup to index previous disk entries.
    ///
    /// # Errors
    /// Returns strategy setup errors, such as failure to create a directory.
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

    /// Insert a value, or replace the value associated with an equal key.
    ///
    /// Compression runs before storage and limit checks. Owned input can avoid
    /// copying on an uncompressed memory write; borrowed input is copied when
    /// the strategy needs ownership. Built-in replacements count only the new
    /// payload and retain the old entry if the replacement returns an error.
    /// Hybrid replacement can change tiers, preferring memory when it fits.
    ///
    /// # Errors
    /// Returns compression, I/O, or [`crate::Error::LimitExceeded`] errors.
    /// Custom strategies may reject replacement. Errors and cancellation do not
    /// imply transactional rollback of all filesystem effects.
    pub async fn put<'a, V>(&mut self, key: K, value: V) -> Result<()>
    where
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value: Cow<'_, [u8]> = self.compressor.compress(value.into()).await?;

        if let Some(entry) = self.data.get_mut(&key) {
            return self.strategy.replace(&key, entry, value).await;
        }

        let entry = self.strategy.put(&key, value).await?;
        self.data.insert(key, entry);
        Ok(())
    }

    /// Read and, if configured, decompress an indexed value without removing it.
    ///
    /// Uncompressed memory reads borrow the stored bytes. Disk reads and
    /// built-in decompression allocate. Hybrid reads do not change tiers.
    /// Use `into_owned()` on the result to retain it after a cache/lock borrow.
    ///
    /// # Errors
    /// Returns [`crate::Error::KeyNotFound`] for a missing index entry, or a
    /// storage/decoding error. Corruption is not always detected.
    pub async fn get(&self, key: K) -> Result<Cow<'_, [u8]>> {
        let entry = self.data.get(&key).ok_or(crate::Error::KeyNotFound)?;
        let value = self.strategy.get(entry).await?;
        self.compressor.decompress(value).await
    }

    /// Remove an entry and return its decompressed bytes as an owned buffer.
    ///
    /// The index entry is removed **before** storage access and decompression.
    /// An error or cancellation can therefore lose access to the entry, leave
    /// an orphaned file, or leave stale capacity accounting; there is no rollback.
    ///
    /// # Errors
    /// Returns [`crate::Error::KeyNotFound`], storage errors, or decompression errors.
    pub async fn take(&mut self, key: K) -> Result<Vec<u8>> {
        let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
        let value = self.strategy.take(entry).await?;
        Ok(self.compressor.decompress(value.into()).await?.into_owned())
    }

    /// Remove an indexed entry and its storage without decompressing it.
    ///
    /// The index entry is removed before storage deletion. An I/O error or
    /// cancellation may leave a file and stale capacity accounting behind.
    ///
    /// # Errors
    /// Returns [`crate::Error::KeyNotFound`] or a strategy/storage error.
    pub async fn delete(&mut self, key: K) -> Result<()> {
        let entry = self.data.remove(&key).ok_or(crate::Error::KeyNotFound)?;
        self.strategy.delete(entry).await
    }

    /// Check the in-memory index without reading storage or verifying integrity.
    ///
    /// `true` does not guarantee a subsequent read will succeed.
    pub fn exists(&self, key: K) -> bool {
        self.data.contains_key(&key)
    }

    /// Snapshot the configured byte limit and tracked stored payload bytes.
    ///
    /// Bytes are counted after compression, excluding keys, metadata, allocation
    /// overhead and disk headers. Memory/disk return `None` without a byte limit;
    /// hybrid returns `Some` only when both tiers have byte limits, summing them.
    /// Entry-only limits are not represented. This is not a system resource quota.
    pub fn capacity(&self) -> Option<CacheCapacity> {
        self.strategy.get_cache_capacity()
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
    /// Rebuild the index from storage, returning the number of recovered entries.
    ///
    /// Call once on a fresh cache before reads/writes. Convert persisted key
    /// strings with `key_from_str`; `None` rejects a file. Built-in strategies
    /// try to move rejected keys/malformed headers to `lost+found`. Files without
    /// the current magic are considered legacy entries, including stray files;
    /// successful indexing does not guarantee that `get` can read them.
    ///
    /// Recovery is best-effort: it does not enforce capacity limits, validate
    /// payload integrity, or decompress data. Reopen with the same key mapping
    /// and codec. Hybrid recovers disk entries only and leaves them on disk.
    /// Repeated recovery can double-count usage and replace indexed entries.
    ///
    /// # Errors
    /// Built-in strategies skip directory-entry listing errors but propagate
    /// directory setup and file read errors. Failure may leave partial strategy
    /// accounting; recreate the instance before another recovery attempt.
    /// See the [restart example](crate#disk-caching-and-recovery-after-restart).
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
    /// Move eligible entries to persistent storage and return the number moved.
    ///
    /// For hybrid storage, moves memory entries to disk and leaves existing disk
    /// entries alone. This is explicit: dropping a cache does not call `flush`.
    /// It neither makes a snapshot nor adds a filesystem durability barrier.
    ///
    /// # Errors
    /// Returns storage or capacity errors. There is no all-or-nothing guarantee:
    /// a failed/cancelled flush can leave disk files and changed accounting while
    /// the index still refers to memory entries. Reserve sufficient disk space
    /// and do not rely on retrying a failed flush to roll back partial work.
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
