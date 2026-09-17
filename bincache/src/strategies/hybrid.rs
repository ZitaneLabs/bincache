use async_trait::async_trait;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use crate::{
    CacheCapacity, DiskUtil, Result,
    traits::{CacheKey, CacheStrategy, FlushableStrategy, RecoverableStrategy},
};

use super::disk::{FORMAT_DIR, recover_entries};

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
pub type DiskEntry = super::disk::Entry;

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
        }
        if let Some(entries_limit) = self.entry_limit {
            if self.current_entry_count + 1 > entries_limit {
                return LimitEvaluation::LimitExceeded(LimitExceededKind::Entries);
            }
        }
        LimitEvaluation::LimitSatisfied
    }

    fn evaluate_replacement(&self, size: usize, old_size: Option<usize>) -> LimitEvaluation {
        let byte_count = self.current_byte_count - old_size.unwrap_or(0) + size;
        if self.byte_limit.is_some_and(|limit| byte_count > limit) {
            return LimitEvaluation::LimitExceeded(LimitExceededKind::Bytes);
        }
        let entry_count = self.current_entry_count - usize::from(old_size.is_some()) + 1;
        if self.entry_limit.is_some_and(|limit| entry_count > limit) {
            return LimitEvaluation::LimitExceeded(LimitExceededKind::Entries);
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
        DiskUtil::create_dir(self.cache_dir.join(FORMAT_DIR)).await
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
            let key = key.to_key();
            let entry = DiskEntry::new(&self.cache_dir, &key, byte_len);
            entry.write(&key, &value).await?;

            // Increment limits
            self.disk_limits.current_byte_count += byte_len;
            self.disk_limits.current_entry_count += 1;

            Ok(Entry::Disk(entry))
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
            Entry::Disk(entry) => Ok(Cow::Owned(entry.read().await?)),
        }
    }

    async fn replace<'a, K, V>(
        &mut self,
        key: &K,
        entry: &mut Self::CacheEntry,
        value: V,
    ) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.len();
        let old_memory_size = match entry {
            Entry::Memory(old) => Some(old.byte_len),
            Entry::Disk(_) => None,
        };
        let old_disk_size = match entry {
            Entry::Memory(_) => None,
            Entry::Disk(old) => Some(old.byte_len),
        };
        let fits_memory = self
            .memory_limits
            .evaluate_replacement(byte_len, old_memory_size);
        let fits_disk = self
            .disk_limits
            .evaluate_replacement(byte_len, old_disk_size);

        if fits_memory.is_satisfied() {
            let new_entry = MemoryEntry {
                data: value.into_owned(),
                byte_len,
            };
            match entry {
                Entry::Memory(old) => {
                    self.memory_limits.current_byte_count =
                        self.memory_limits.current_byte_count - old.byte_len + byte_len;
                    *old = new_entry;
                }
                Entry::Disk(old) => {
                    DiskUtil::delete(&old.path).await?;
                    self.disk_limits.current_byte_count -= old.byte_len;
                    self.disk_limits.current_entry_count -= 1;
                    self.memory_limits.current_byte_count += byte_len;
                    self.memory_limits.current_entry_count += 1;
                    *entry = Entry::Memory(new_entry);
                }
            }
            return Ok(());
        }

        if fits_disk.is_satisfied() {
            let key = key.to_key();
            match entry {
                Entry::Memory(old) => {
                    let new_entry = DiskEntry::new(&self.cache_dir, &key, byte_len);
                    new_entry.write(&key, &value).await?;
                    self.memory_limits.current_byte_count -= old.byte_len;
                    self.memory_limits.current_entry_count -= 1;
                    self.disk_limits.current_byte_count += byte_len;
                    self.disk_limits.current_entry_count += 1;
                    *entry = Entry::Disk(new_entry);
                }
                Entry::Disk(old) => {
                    old.write(&key, &value).await?;
                    self.disk_limits.current_byte_count =
                        self.disk_limits.current_byte_count - old.byte_len + byte_len;
                    old.byte_len = byte_len;
                }
            }
            return Ok(());
        }

        let limit_kind = Cow::Borrowed(match fits_disk {
            LimitEvaluation::LimitExceeded(LimitExceededKind::Bytes) => LIMIT_KIND_BYTE_DISK,
            LimitEvaluation::LimitExceeded(LimitExceededKind::Entries) => LIMIT_KIND_ENTRY_DISK,
            _ => unreachable!(),
        });
        Err(crate::Error::LimitExceeded { limit_kind })
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
                let data = entry.read().await?;

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
    async fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        let entries = recover_entries(&self.cache_dir, recover_key).await?;
        for (_, entry) in &entries {
            self.disk_limits.current_byte_count += entry.byte_len;
        }
        self.disk_limits.current_entry_count += entries.len();
        Ok(entries
            .into_iter()
            .map(|(key, entry)| (key, Entry::Disk(entry)))
            .collect())
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
        let key = key.to_key();
        let disk_entry = DiskEntry::new(&self.cache_dir, &key, entry.byte_len);
        disk_entry.write(&key, &entry.data).await?;

        // Increment limits
        self.disk_limits.current_byte_count += entry.byte_len;
        self.disk_limits.current_entry_count += 1;

        // Return new disk entry
        Ok(Some(Entry::Disk(disk_entry)))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{FORMAT_DIR, Hybrid, LIMIT_KIND_BYTE_DISK, LIMIT_KIND_ENTRY_DISK, Limits};
    use crate::strategies::disk::cache_path;
    use crate::{Cache, Error, NO_COMPRESSION, async_test, utils::test::TempDir};

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

            assert!(cache_path(temp_dir.as_ref(), "baz").is_file());
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

            assert!(cache_path(temp_dir.as_ref(), "baz").is_file());
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

            assert!(cache_path(temp_dir.as_ref(), "baz").is_file());
            assert!(cache_path(temp_dir.as_ref(), "bax").is_file());

            assert!(matches!(
                cache.put("quix", b"quix".to_vec()).await,
                Err(Error::LimitExceeded { limit_kind }) if limit_kind == LIMIT_KIND_BYTE_DISK
            ));
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

            assert!(cache_path(temp_dir.as_ref(), "baz").is_file());
            assert!(cache_path(temp_dir.as_ref(), "bax").is_file());

            assert!(matches!(
                cache.put("quix", b"quix".to_vec()).await,
                Err(Error::LimitExceeded { limit_kind }) if limit_kind == LIMIT_KIND_ENTRY_DISK
            ));
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

        async fn test_safe_disk_key() {
            let temp_dir = TempDir::new();
            let escaped = temp_dir.as_ref().with_extension("escaped");
            let key = format!("../{}", escaped.file_name().unwrap().to_string_lossy());
            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(Some(0), None),
                Limits::default(),
            ), NO_COMPRESSION).await.unwrap();
            cache.put(key, b"safe".to_vec()).await.unwrap();
            assert!(!escaped.exists());
            assert_eq!(fs::read_dir(temp_dir.as_ref()).unwrap().count(), 1);
            assert_eq!(fs::read_dir(temp_dir.as_ref().join(FORMAT_DIR)).unwrap().count(), 1);
        }

        async fn test_replace_accounting_capacity_and_failure() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(Some(100), None),
                Limits::new(Some(0), None),
            ), NO_COMPRESSION).await.unwrap();
            cache.put("foo", vec![1; 100]).await.unwrap();
            cache.put("foo", vec![2; 50]).await.unwrap();
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().memory_limits.current_byte_count, 50);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 1);
            cache.put("foo", vec![5; 75]).await.unwrap();
            assert_eq!(cache.strategy().memory_limits.current_byte_count, 75);
            cache.put("foo", vec![2; 50]).await.unwrap();
            cache.put("bar", vec![3; 50]).await.unwrap();
            assert!(cache.put("foo", vec![4; 51]).await.is_err());
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().memory_limits.current_byte_count, 100);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 2);
        }

        async fn test_replace_disk_backed_entry() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(Some(0), None),
                Limits::new(Some(100), None),
            ), NO_COMPRESSION).await.unwrap();
            cache.put("foo", vec![1; 100]).await.unwrap();
            cache.put("foo", vec![2; 50]).await.unwrap();
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 50);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 1);
            cache.put("bar", vec![3; 50]).await.unwrap();
            assert!(cache.put("foo", vec![4; 51]).await.is_err());
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 100);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 2);
        }

        async fn test_recovery_ignores_old_formats() {
            crate::strategies::recovery_tests::ignores_old_formats(
                |path| Hybrid::new(path, Limits::new(Some(0), Some(0)), Limits::new(None, Some(1))),
                |hybrid| (hybrid.disk_limits.current_byte_count, hybrid.disk_limits.current_entry_count),
            ).await;
        }

        async fn test_interrupted_write_recovery() {
            crate::strategies::recovery_tests::interrupted(
                |path| Hybrid::new(path, Limits::new(Some(0), Some(0)), Limits::default()),
                |hybrid| (hybrid.disk_limits.current_byte_count, hybrid.disk_limits.current_entry_count),
            ).await;
        }
    }
}
