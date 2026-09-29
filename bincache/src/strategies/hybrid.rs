use async_trait::async_trait;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use super::{
    Limits,
    disk::{FORMAT_DIR, recover_entries},
};
use crate::{
    CacheCapacity, DiskUtil, Result,
    traits::{CacheKey, CacheStrategy, FlushableStrategy, RecoverableStrategy},
};

const LIMIT_KIND_BYTE_DISK: &str = "Stored bytes on disk";
const LIMIT_KIND_ENTRY_DISK: &str = "Stored entries on disk";
const LIMIT_KINDS: [&str; 2] = [LIMIT_KIND_BYTE_DISK, LIMIT_KIND_ENTRY_DISK];

/// A cache entry stored in memory.
#[derive(Debug)]
pub struct MemoryEntry {
    data: Vec<u8>,
}

/// A cache entry stored on disk.
pub type DiskEntry = super::disk::Entry;

/// A hybrid cache entry.
#[derive(Debug)]
pub enum Entry {
    Memory(MemoryEntry),
    Disk(DiskEntry),
}

impl Entry {
    fn len(&self) -> usize {
        match self {
            Self::Memory(entry) => entry.data.len(),
            Self::Disk(entry) => entry.byte_len,
        }
    }
}

/// Hybrid cache strategy.
///
/// Stores entries in memory, falling back to disk when memory limits are reached.
#[derive(Debug)]
pub struct Hybrid {
    cache_dir: PathBuf,
    memory_limits: Limits,
    disk_limits: Limits,
}

impl Default for Hybrid {
    fn default() -> Self {
        Self::new(Path::new("cache"), Limits::default(), Limits::default())
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

    fn limits(&mut self, entry: &Entry) -> &mut Limits {
        match entry {
            Entry::Memory(_) => &mut self.memory_limits,
            Entry::Disk(_) => &mut self.disk_limits,
        }
    }

    fn replace_entry(&mut self, entry: &mut Entry, replacement: Entry) {
        self.limits(entry).remove(entry.len());
        self.limits(&replacement).add(replacement.len());
        *entry = replacement;
    }
}

#[async_trait]
impl CacheStrategy for Hybrid {
    type CacheEntry = Entry;

    async fn setup(&mut self) -> Result<()> {
        DiskUtil::create_dir(self.cache_dir.join(FORMAT_DIR)).await
    }

    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Entry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let entry = if self
            .memory_limits
            .check(value.len(), None, LIMIT_KINDS)
            .is_ok()
        {
            Entry::Memory(MemoryEntry {
                data: value.into_owned(),
            })
        } else {
            self.disk_limits.check(value.len(), None, LIMIT_KINDS)?;
            let entry = DiskEntry::new(&self.cache_dir, &key.to_key(), value.len());
            entry.write(&value).await?;
            Entry::Disk(entry)
        };
        self.limits(&entry).add(entry.len());
        Ok(entry)
    }

    async fn get<'a>(&self, entry: &'a Entry) -> Result<Cow<'a, [u8]>> {
        match entry {
            Entry::Memory(entry) => Ok(Cow::Borrowed(&entry.data)),
            Entry::Disk(entry) => Ok(Cow::Owned(entry.read().await?)),
        }
    }

    async fn replace<'a, K, V>(&mut self, key: &K, entry: &mut Entry, value: V) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let old_memory = matches!(entry, Entry::Memory(_)).then(|| entry.len());
        if self
            .memory_limits
            .check(value.len(), old_memory, LIMIT_KINDS)
            .is_ok()
        {
            if let Entry::Disk(old) = entry {
                DiskUtil::delete(&old.path).await?;
            }
            self.replace_entry(
                entry,
                Entry::Memory(MemoryEntry {
                    data: value.into_owned(),
                }),
            );
        } else {
            let old_disk = matches!(entry, Entry::Disk(_)).then(|| entry.len());
            self.disk_limits.check(value.len(), old_disk, LIMIT_KINDS)?;
            match entry {
                Entry::Memory(_) => {
                    let disk = DiskEntry::new(&self.cache_dir, &key.to_key(), value.len());
                    disk.write(&value).await?;
                    self.replace_entry(entry, Entry::Disk(disk));
                }
                Entry::Disk(old) => {
                    old.write(&value).await?;
                    self.disk_limits.current_byte_count =
                        self.disk_limits.current_byte_count - old.byte_len + value.len();
                    old.byte_len = value.len();
                }
            }
        }
        Ok(())
    }

    async fn take(&mut self, entry: Entry) -> Result<Vec<u8>> {
        match entry {
            Entry::Memory(memory) => {
                self.memory_limits.remove(memory.data.len());
                Ok(memory.data)
            }
            Entry::Disk(disk) => {
                let data = disk.read().await?;
                DiskUtil::delete(&disk.path).await?;
                self.disk_limits.remove(disk.byte_len);
                Ok(data)
            }
        }
    }

    async fn delete(&mut self, entry: Entry) -> Result<()> {
        if let Entry::Disk(disk) = &entry {
            DiskUtil::delete(&disk.path).await?;
        }
        self.limits(&entry).remove(entry.len());
        Ok(())
    }

    fn get_cache_capacity(&self) -> Option<CacheCapacity> {
        Some(CacheCapacity::new(
            self.memory_limits.byte_limit? + self.disk_limits.byte_limit?,
            self.memory_limits.current_byte_count + self.disk_limits.current_byte_count,
        ))
    }
}

#[async_trait]
impl RecoverableStrategy for Hybrid {
    async fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Entry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        let entries = recover_entries(&self.cache_dir, recover_key).await?;
        for (_, entry) in &entries {
            self.disk_limits.add(entry.byte_len);
        }
        Ok(entries
            .into_iter()
            .map(|(key, entry)| (key, Entry::Disk(entry)))
            .collect())
    }
}

#[async_trait]
impl FlushableStrategy for Hybrid {
    async fn flush<K>(&mut self, key: &K, entry: &Entry) -> Result<Option<Entry>>
    where
        K: CacheKey + Sync + Send,
    {
        let Entry::Memory(memory) = entry else {
            return Ok(None);
        };
        self.disk_limits
            .check(memory.data.len(), None, LIMIT_KINDS)?;
        let disk = DiskEntry::new(&self.cache_dir, &key.to_key(), memory.data.len());
        disk.write(&memory.data).await?;
        self.disk_limits.add(disk.byte_len);
        Ok(Some(Entry::Disk(disk)))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{FORMAT_DIR, Hybrid, LIMIT_KIND_BYTE_DISK, LIMIT_KIND_ENTRY_DISK, Limits};
    use crate::strategies::disk::cache_path;
    use crate::{Cache, Error, NO_COMPRESSION, async_test, utils::test::TempDir};

    use crate::strategies::test_helpers;

    fn memory_counts(strategy: &Hybrid) -> (usize, usize) {
        assert_eq!(
            (
                strategy.disk_limits.current_byte_count,
                strategy.disk_limits.current_entry_count
            ),
            (0, 0)
        );
        (
            strategy.memory_limits.current_byte_count,
            strategy.memory_limits.current_entry_count,
        )
    }

    async_test! {
        async fn test_default_strategy() {
            test_helpers::basic(Hybrid::default(), memory_counts).await;
        }

        async fn test_memory_and_disk_limits() {
            for (bytes, entries, expected) in [
                (Some(6), None, LIMIT_KIND_BYTE_DISK),
                (None, Some(2), LIMIT_KIND_ENTRY_DISK),
            ] {
                for bounded_disk in [false, true] {
                    let dir = TempDir::new();
                    let disk = if bounded_disk { Limits::new(bytes, entries) } else { Limits::default() };
                    let mut cache = Cache::new(Hybrid::new(
                        dir.as_ref(), Limits::new(bytes, entries), disk,
                    ), NO_COMPRESSION).await.unwrap();
                    for key in ["foo", "bar"] {
                        cache.put(key, key.as_bytes()).await.unwrap();
                    }
                    for key in ["foo", "bar"] {
                        assert_eq!(cache.get(key).await.unwrap(), key.as_bytes());
                    }
                    for key in ["baz", "bax"] {
                        cache.put(key, key.as_bytes()).await.unwrap();
                        assert!(cache_path(dir.as_ref(), key).is_file());
                    }
                    let result = cache.put("quix", b"quix".as_slice()).await;
                    if bounded_disk {
                        assert!(matches!(result, Err(Error::LimitExceeded { limit_kind }) if limit_kind == expected));
                    } else {
                        result.unwrap();
                    }
                }
            }
        }

        async fn test_recovery() {
            let dir = TempDir::new();
            test_helpers::recovery(
                Hybrid::new(dir.as_ref(), Limits::new(None, Some(1)), Limits::default()),
                Hybrid::new(dir.as_ref(), Limits::default(), Limits::default()),
                |strategy| (strategy.disk_limits.current_byte_count, strategy.disk_limits.current_entry_count),
                &["foo", "bar", "baz"], &["bar", "baz"],
            ).await;
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
            let dir = TempDir::new();
            test_helpers::replacement(Hybrid::new(dir.as_ref(), Limits::new(Some(100), None), Limits::new(Some(0), None)), memory_counts).await;
        }

        async fn test_replace_disk_backed_entry() {
            let dir = TempDir::new();
            test_helpers::replacement(
                Hybrid::new(dir.as_ref(), Limits::new(Some(0), None), Limits::new(Some(100), None)),
                |strategy| (strategy.disk_limits.current_byte_count, strategy.disk_limits.current_entry_count),
            ).await;
        }

        async fn test_disk_to_memory_replacement_error_and_retry() {
            let dir = TempDir::new();
            let mut cache = Cache::new(Hybrid::new(
                dir.as_ref(),
                Limits::new(Some(10), Some(1)),
                Limits::new(Some(10), Some(1)),
            ), NO_COMPRESSION).await.unwrap();
            cache.put("a", b"a".to_vec()).await.unwrap();
            cache.put("b", b"old".to_vec()).await.unwrap();
            cache.delete("a").await.unwrap();

            // A directory at the file path forces deletion to return an error.
            let path = cache_path(dir.as_ref(), "b");
            let backup = path.with_extension("backup");
            fs::rename(&path, &backup).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(cache.put("b", b"new value".to_vec()).await.is_err());
            assert_eq!(cache.strategy().memory_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 3);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 1);
            fs::remove_dir(&path).unwrap();
            fs::rename(&backup, &path).unwrap();
            assert_eq!(cache.get("b").await.unwrap(), b"old".as_slice());

            cache.put("b", b"new value".to_vec()).await.unwrap();
            assert_eq!(cache.get("b").await.unwrap(), b"new value".as_slice());
            assert!(!path.exists());
            assert_eq!(cache.strategy().memory_limits.current_byte_count, 9);
            assert_eq!(cache.strategy().memory_limits.current_entry_count, 1);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);
        }

        async fn test_replace_normalized_recovered_key() {
            crate::strategies::recovery_tests::replaces_normalized_key(
                |path| Hybrid::new(path, Limits::new(Some(0), Some(0)), Limits::new(None, Some(1))),
                |hybrid| (hybrid.disk_limits.current_byte_count, hybrid.disk_limits.current_entry_count),
            ).await;
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
