use async_trait::async_trait;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use crate::{
    traits::{CacheKey, CacheStrategy, FlushableStrategy, RecoverableStrategy},
    CacheCapacity, DiskUtil, Result,
};

const LIMIT_KIND_BYTE_DISK: &str = "Stored bytes on disk";
const LIMIT_KIND_ENTRY_DISK: &str = "Stored entries on disk";

/// The limit kind that was exceeded.
enum LimitExceededKind {
    /// Exceeded byte limit.
    Bytes,
    /// Exceeded entry limit.
    Entries,
}

/// The result of evaluating a byte size against a limit.
enum LimitEvaluation {
    LimitSatisfied,
    LimitExceeded(LimitExceededKind),
}

impl LimitEvaluation {
    /// Returns true if the limit was satisfied.
    fn is_satisfied(&self) -> bool {
        matches!(self, LimitEvaluation::LimitSatisfied)
    }
}

/// A cache entry stored in memory.
#[derive(Debug)]
pub struct MemoryEntry {
    data: Vec<u8>,
    byte_len: usize,
}

/// A cache entry stored on disk.
#[derive(Debug)]
pub struct DiskEntry {
    path: PathBuf,
    byte_len: usize,
}

/// A hybrid cache entry.
#[derive(Debug)]
pub enum Entry {
    Memory(MemoryEntry),
    Disk(DiskEntry),
}

#[derive(Debug, Default)]
pub struct Limits {
    /// The maximum number of bytes that can be stored.
    byte_limit: Option<usize>,
    /// The maximum number of entries that can be stored.
    entry_limit: Option<usize>,
    /// The current number of bytes stored.
    current_byte_count: usize,
    /// The current number of entries stored.
    current_entry_count: usize,
}

impl Limits {
    pub fn new(byte_limit: Option<usize>, entry_limit: Option<usize>) -> Self {
        Self {
            byte_limit,
            entry_limit,
            ..Default::default()
        }
    }

    fn evaluate(&self, size: usize) -> LimitEvaluation {
        if let Some(byte_limit) = self.byte_limit {
            if self.current_byte_count + size > byte_limit {
                return LimitEvaluation::LimitExceeded(LimitExceededKind::Bytes);
            }
        } else if let Some(entries_limit) = self.entry_limit {
            if self.current_entry_count + 1 > entries_limit {
                return LimitEvaluation::LimitExceeded(LimitExceededKind::Entries);
            }
        }
        LimitEvaluation::LimitSatisfied
    }
}

/// Hybrid cache strategy.
///
/// This strategy stores entries on memory and flushed entries to disk if memory doesn't suffice.
/// It can be configured to limit the number of bytes and/or entries that can be stored.
#[derive(Debug)]
pub struct Hybrid {
    /// The directory where entries are stored.
    cache_dir: PathBuf,
    /// Memory usage limits.
    memory_limits: Limits,
    /// Disk usage limits.
    disk_limits: Limits,
}

impl Default for Hybrid {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("cache"),
            memory_limits: Limits::default(),
            disk_limits: Limits::default(),
        }
    }
}

impl Hybrid {
    pub fn new<'a>(
        cache_dir: impl Into<Cow<'a, Path>>,
        memory_limits: Limits,
        disk_limits: Limits,
    ) -> Self {
        Self {
            cache_dir: cache_dir.into().into_owned(),
            memory_limits,
            disk_limits,
        }
    }
}

#[async_trait]
impl CacheStrategy for Hybrid {
    type CacheEntry = Entry;

    async fn setup(&mut self) -> Result<()> {
        DiskUtil::create_dir(&self.cache_dir).await
    }

    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.as_ref().len();

        // Evaluate limits
        let fits_into_memory = self.memory_limits.evaluate(byte_len);
        let fits_into_disk = self.disk_limits.evaluate(byte_len);

        // Try to store in memory
        if fits_into_memory.is_satisfied() {
            // Increment limits
            self.memory_limits.current_byte_count += byte_len;
            self.memory_limits.current_entry_count += 1;

            Ok(Entry::Memory(MemoryEntry {
                data: value.into_owned(),
                byte_len,
            }))
        }
        // Try to store on disk
        else if fits_into_disk.is_satisfied() {
            // Write to disk
            let path = self.cache_dir.join(key.to_key());
            DiskUtil::write(&path, &value).await?;

            // Increment limits
            self.disk_limits.current_byte_count += byte_len;
            self.disk_limits.current_entry_count += 1;

            Ok(Entry::Disk(DiskEntry { path, byte_len }))
        }
        // Return limit exceeded error
        else {
            use LimitEvaluation::LimitExceeded;
            let limit_kind = Cow::Borrowed(match fits_into_disk {
                LimitExceeded(LimitExceededKind::Bytes) => LIMIT_KIND_BYTE_DISK,
                LimitExceeded(LimitExceededKind::Entries) => LIMIT_KIND_ENTRY_DISK,
                _ => unreachable!(),
            });
            Err(crate::Error::LimitExceeded { limit_kind })
        }
    }

    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        match entry {
            Entry::Memory(entry) => Ok(Cow::Borrowed(&entry.data)),
            Entry::Disk(entry) => Ok(Cow::Owned(
                DiskUtil::read(&entry.path, Some(entry.byte_len)).await?,
            )),
        }
    }

    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        match entry {
            Entry::Memory(entry) => {
                // Decrement limits
                self.memory_limits.current_byte_count -= entry.byte_len;
                self.memory_limits.current_entry_count -= 1;

                Ok(entry.data)
            }
            Entry::Disk(ref entry) => {
                let data = DiskUtil::read(&entry.path, Some(entry.byte_len)).await?;

                // Delete from disk
                DiskUtil::delete(&entry.path).await?;

                // Decrement limits
                self.disk_limits.current_byte_count -= entry.byte_len;
                self.disk_limits.current_entry_count -= 1;

                Ok(data)
            }
        }
    }

    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        match entry {
            Entry::Memory(entry) => {
                // Decrement limits
                self.memory_limits.current_byte_count -= entry.byte_len;
                self.memory_limits.current_entry_count -= 1;
            }
            Entry::Disk(entry) => {
                // Delete from disk
                DiskUtil::delete(&entry.path).await?;

                // Decrement limits
                self.disk_limits.current_byte_count -= entry.byte_len;
                self.disk_limits.current_entry_count -= 1;
            }
        }
        Ok(())
    }

    fn get_cache_capacity(&self) -> Option<CacheCapacity> {
        if let (Some(memory_byte_limit), Some(disk_byte_limit)) =
            (self.memory_limits.byte_limit, self.disk_limits.byte_limit)
        {
            Some(CacheCapacity::new(
                memory_byte_limit + disk_byte_limit,
                self.memory_limits.current_byte_count + self.disk_limits.current_byte_count,
            ))
        } else {
            None
        }
    }
}

#[async_trait]
impl RecoverableStrategy for Hybrid {
    async fn recover<K, F>(&mut self, mut recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        // Create the `lost+found` directory
        let lost_found_dir = self.cache_dir.join("lost+found");
        std::fs::create_dir_all(&lost_found_dir)?;

        // Closure to move files to the `lost+found` directory
        let move_to_lost_found = |source: &Path| {
            // We explcitly ignore any errors here, as we don't want to fail
            // the entire recovery process because of a single file.
            let Some(file_name) = source.file_name() else {
                return;
            };
            let target_path = lost_found_dir.join(file_name);
            _ = std::fs::rename(source, target_path);
        };

        // Iterate over all files in the cache directory
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.cache_dir)?.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // If key recovery fails, we move the entry to the `lost+found` directory.
            let Some(key) = path
                .file_name()
                .and_then(|p| p.to_str())
                .and_then(&mut recover_key)
            else {
                move_to_lost_found(&path);
                continue;
            };

            // Read file
            let buf = DiskUtil::read(&path, None).await?;

            // Increment limits
            self.disk_limits.current_byte_count += buf.len();
            self.disk_limits.current_entry_count += 1;

            // Push entry
            entries.push((
                key,
                Entry::Disk(DiskEntry {
                    path,
                    byte_len: buf.len(),
                }),
            ));
        }

        // Return recovered entries
        Ok(entries)
    }
}

#[async_trait]
impl FlushableStrategy for Hybrid {
    async fn flush<K>(
        &mut self,
        key: &K,
        entry: &Self::CacheEntry,
    ) -> Result<Option<Self::CacheEntry>>
    where
        K: CacheKey + Sync + Send,
    {
        // We can only flush entries stored in memory
        let Self::CacheEntry::Memory(entry) = entry else {
            return Ok(None);
        };

        // Check if entry fits into disk
        if let LimitEvaluation::LimitExceeded(reason) = self.disk_limits.evaluate(entry.byte_len) {
            let limit_kind = Cow::Borrowed(match reason {
                LimitExceededKind::Bytes => LIMIT_KIND_BYTE_DISK,
                LimitExceededKind::Entries => LIMIT_KIND_ENTRY_DISK,
            });
            return Err(crate::Error::LimitExceeded { limit_kind });
        }

        // Write to disk
        let path = self.cache_dir.join(key.to_key());
        DiskUtil::write(&path, &entry.data).await?;

        // Increment limits
        self.disk_limits.current_byte_count += entry.byte_len;
        self.disk_limits.current_entry_count += 1;

        // Return new disk entry
        Ok(Some(Entry::Disk(DiskEntry {
            path,
            byte_len: entry.byte_len,
        })))
    }
}

#[cfg(test)]
mod tests {
    use std::fs::metadata;

    use super::{Hybrid, Limits, LIMIT_KIND_BYTE_DISK, LIMIT_KIND_ENTRY_DISK};
    use crate::{async_test, utils::test::TempDir, Cache, Error, NO_COMPRESSION};

    async_test! {
        async fn test_default_strategy() {
            // We don't need a temp dir here, because we don't write to disk
            let mut cache = Cache::new(Hybrid::default(), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".to_vec()).await.unwrap();

            assert_eq!(cache.strategy().memory_limits.current_byte_count, 3);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 1);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);

            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.strategy().memory_limits.current_byte_count, 6);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 2);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            assert!(cache.get("baz").await.is_err());

            cache.delete("foo").await.unwrap();

            assert_eq!(cache.strategy().memory_limits.current_byte_count, 3);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 1);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);

            cache.delete("bar").await.unwrap();

            assert_eq!(cache.strategy().memory_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);
        }

        async fn test_strategy_with_memory_byte_limit() {
            let temp_dir = TempDir::new();

            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(Some(6), None),
                Limits::default(),
            ), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            cache.put("baz", b"baz".to_vec()).await.unwrap();

            assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
        }

        async fn test_strategy_with_memory_entry_limit() {
            let temp_dir = TempDir::new();

            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(None, Some(2)),
                Limits::default(),
            ), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            cache.put("baz", b"baz".to_vec()).await.unwrap();

            assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
        }

        async fn test_strategy_with_memory_and_disk_byte_limit() {
            let temp_dir = TempDir::new();

            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(Some(6), None),
                Limits::new(Some(6), None),
            ), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            cache.put("baz", b"baz".to_vec()).await.unwrap();
            cache.put("bax", b"bax".to_vec()).await.unwrap();

            assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
            assert!(metadata(temp_dir.as_ref().join("bax")).unwrap().is_file());

            match cache.put("quix", b"quix".to_vec()).await {
                Err(err) => match err {
                    Error::LimitExceeded { limit_kind } => {
                        assert_eq!(limit_kind, LIMIT_KIND_BYTE_DISK);
                    }
                    _ => {
                        panic!("Unexpected error: {:?}", err);
                    }
                },
                _ => (),
            }
        }

        async fn test_strategy_with_memory_and_disk_entry_limit() {
            let temp_dir = TempDir::new();

            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(None, Some(2)),
                Limits::new(None, Some(2)),
            ), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            cache.put("baz", b"baz".to_vec()).await.unwrap();
            cache.put("bax", b"bax".to_vec()).await.unwrap();

            assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
            assert!(metadata(temp_dir.as_ref().join("bax")).unwrap().is_file());

            match cache.put("quix", b"quix".to_vec()).await {
                Err(err) => match err {
                    Error::LimitExceeded { limit_kind } => {
                        assert_eq!(limit_kind, LIMIT_KIND_ENTRY_DISK);
                    }
                    _ => {
                        panic!("Unexpected error: {:?}", err);
                    }
                },
                _ => (),
            }
        }

        async fn test_recovery() {
            let temp_dir = TempDir::new();

            // populate cache
            {
                let mut cache = Cache::new(Hybrid::new(
                    temp_dir.as_ref(),
                    Limits::new(None, Some(1)),
                    Limits::default(),
                ), NO_COMPRESSION).await.unwrap();

                cache.put("foo", b"foo".to_vec()).await.unwrap();
                cache.put("bar", b"bar".to_vec()).await.unwrap();
                cache.put("baz", b"baz".to_vec()).await.unwrap();
            }

            // recover cache
            {
                let mut cache = Cache::new(Hybrid::new(
                    temp_dir.as_ref(),
                    Limits::default(),
                    Limits::default(),
                ), NO_COMPRESSION).await.unwrap();
                let recovered_items = cache
                    .recover(|k| Some(k.to_string()))
                    .await
                    .expect("Failed to recover");

                assert_eq!(recovered_items, 2);
                assert_eq!(cache.strategy().disk_limits.current_byte_count, 6);
                assert_eq!(cache.strategy().disk_limits.current_entry_count, 2);
            }
        }

        async fn test_flush() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::default(),
                Limits::default(),
            ), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".as_slice()).await.unwrap();
            cache.put("bar", b"bar".as_slice()).await.unwrap();

            assert_eq!(cache.strategy().memory_limits.current_byte_count, 6);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 2);

            cache.flush().await.unwrap();

            assert_eq!(cache.strategy().memory_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 6);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 2);
        }
    }
}
