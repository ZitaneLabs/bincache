use std::{
    borrow::Cow,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::{
    traits::{CacheKey, CacheStrategy},
    Result,
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
pub struct MemoryEntry {
    data: Vec<u8>,
    byte_len: usize,
}

/// A cache entry stored on disk.
pub struct DiskEntry {
    path: PathBuf,
    byte_len: usize,
}

/// A hybrid cache entry.
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
#[derive(Debug, Default)]
pub struct Hybrid {
    /// The directory where entries are stored.
    cache_dir: PathBuf,
    /// Memory usage limits.
    memory_limits: Limits,
    /// Disk usage limits.
    disk_limits: Limits,
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

    fn read_from_disk(&self, entry: &DiskEntry) -> Result<Vec<u8>> {
        let mut file = File::open(&entry.path)?;
        let mut buf = Vec::with_capacity(entry.byte_len);
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn write_to_disk(&self, path: impl AsRef<Path>, value: &[u8]) -> Result<()> {
        let mut file = File::create(path)?;
        file.write_all(&value)?;
        file.sync_data()?;
        Ok(())
    }

    fn delete_from_disk(&self, entry: &DiskEntry) -> Result<()> {
        Ok(std::fs::remove_file(&entry.path)?)
    }
}

impl CacheStrategy for Hybrid {
    type CacheEntry = Entry;

    fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        F: Fn(&str) -> Option<K>,
    {
        // Create the `lost+found` directory
        let lost_found_dir = self.cache_dir.join("lost+found");
        std::fs::create_dir_all(&lost_found_dir)?;

        // Closure to move files to the `lost+found` directory
        let move_to_lost_found = |source: &Path| {
            // We explcitly ignore any errors here, as we don't want to fail
            // the entire recovery process because of a single file.
            let Some(file_name) = source.file_name() else { return };
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
            let Some(key) = path.file_name().and_then(|p| p.to_str()).and_then(|s| recover_key(s)) else {
                move_to_lost_found(&path);
                continue
            };

            // Read file
            let mut file = File::open(&path)?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;

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

    fn put<'a>(
        &mut self,
        key: &impl CacheKey,
        value: impl Into<Cow<'a, [u8]>>,
    ) -> Result<Self::CacheEntry> {
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
            self.write_to_disk(&path, &value)?;

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

    fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        match entry {
            Entry::Memory(entry) => Ok(Cow::Borrowed(&entry.data)),
            Entry::Disk(entry) => Ok(Cow::Owned(self.read_from_disk(entry)?)),
        }
    }

    fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        match entry {
            Entry::Memory(entry) => {
                // Decrement limits
                self.memory_limits.current_byte_count -= entry.byte_len;
                self.memory_limits.current_entry_count -= 1;

                Ok(entry.data)
            }
            Entry::Disk(ref entry) => {
                let data = self.read_from_disk(entry)?;

                // Delete from disk
                self.delete_from_disk(entry)?;

                // Decrement limits
                self.disk_limits.current_byte_count -= entry.byte_len;
                self.disk_limits.current_entry_count -= 1;

                Ok(data)
            }
        }
    }

    fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        match entry {
            Entry::Memory(entry) => {
                // Decrement limits
                self.memory_limits.current_byte_count -= entry.byte_len;
                self.memory_limits.current_entry_count -= 1;
            }
            Entry::Disk(entry) => {
                // Delete from disk
                self.delete_from_disk(&entry)?;

                // Decrement limits
                self.disk_limits.current_byte_count -= entry.byte_len;
                self.disk_limits.current_entry_count -= 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::metadata;

    use super::{Hybrid, Limits, LIMIT_KIND_BYTE_DISK, LIMIT_KIND_ENTRY_DISK};
    use crate::{test_utils::TempDir, Cache, Error};

    #[test]
    fn test_default_strategy() {
        // We don't need a temp dir here, because we don't write to disk
        let mut cache = Cache::new(Hybrid::default());

        cache.put("foo", b"foo".to_vec()).unwrap();

        assert_eq!(cache.strategy().memory_limits.current_byte_count, 3);
        assert_eq!(cache.strategy().memory_limits.current_entry_count, 1);
        assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
        assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);

        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.strategy().memory_limits.current_byte_count, 6);
        assert_eq!(cache.strategy().memory_limits.current_entry_count, 2);
        assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
        assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        assert!(cache.get("baz").is_err());

        cache.delete("foo").unwrap();

        assert_eq!(cache.strategy().memory_limits.current_byte_count, 3);
        assert_eq!(cache.strategy().memory_limits.current_entry_count, 1);
        assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
        assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);

        cache.delete("bar").unwrap();

        assert_eq!(cache.strategy().memory_limits.current_byte_count, 0);
        assert_eq!(cache.strategy().memory_limits.current_entry_count, 0);
        assert_eq!(cache.strategy().disk_limits.current_byte_count, 0);
        assert_eq!(cache.strategy().disk_limits.current_entry_count, 0);
    }

    #[test]
    fn test_strategy_with_memory_byte_limit() {
        let temp_dir = TempDir::new();

        let mut cache = Cache::new(Hybrid::new(
            temp_dir.as_ref(),
            Limits::new(Some(6), None),
            Limits::default(),
        ));

        cache.put("foo", b"foo".to_vec()).unwrap();
        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        cache.put("baz", b"baz".to_vec()).unwrap();

        assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
    }

    #[test]
    fn test_strategy_with_memory_entry_limit() {
        let temp_dir = TempDir::new();

        let mut cache = Cache::new(Hybrid::new(
            temp_dir.as_ref(),
            Limits::new(None, Some(2)),
            Limits::default(),
        ));

        cache.put("foo", b"foo".to_vec()).unwrap();
        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        cache.put("baz", b"baz".to_vec()).unwrap();

        assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
    }

    #[test]
    fn test_strategy_with_memory_and_disk_byte_limit() {
        let temp_dir = TempDir::new();

        let mut cache = Cache::new(Hybrid::new(
            temp_dir.as_ref(),
            Limits::new(Some(6), None),
            Limits::new(Some(6), None),
        ));

        cache.put("foo", b"foo".to_vec()).unwrap();
        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        cache.put("baz", b"baz".to_vec()).unwrap();
        cache.put("bax", b"bax".to_vec()).unwrap();

        assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
        assert!(metadata(temp_dir.as_ref().join("bax")).unwrap().is_file());

        match cache.put("quix", b"quix".to_vec()) {
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

    #[test]
    fn test_strategy_with_memory_and_disk_entry_limit() {
        let temp_dir = TempDir::new();

        let mut cache = Cache::new(Hybrid::new(
            temp_dir.as_ref(),
            Limits::new(None, Some(2)),
            Limits::new(None, Some(2)),
        ));

        cache.put("foo", b"foo".to_vec()).unwrap();
        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        cache.put("baz", b"baz".to_vec()).unwrap();
        cache.put("bax", b"bax".to_vec()).unwrap();

        assert!(metadata(temp_dir.as_ref().join("baz")).unwrap().is_file());
        assert!(metadata(temp_dir.as_ref().join("bax")).unwrap().is_file());

        match cache.put("quix", b"quix".to_vec()) {
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

    #[test]
    fn test_recovery() {
        let temp_dir = TempDir::new();

        // populate cache
        {
            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::new(None, Some(1)),
                Limits::default(),
            ));

            cache.put("foo", b"foo".to_vec()).unwrap();
            cache.put("bar", b"bar".to_vec()).unwrap();
            cache.put("baz", b"baz".to_vec()).unwrap();
        }

        // recover cache
        {
            let mut cache = Cache::new(Hybrid::new(
                temp_dir.as_ref(),
                Limits::default(),
                Limits::default(),
            ));
            let recovered_items = cache
                .recover(|k| Some(k.to_string()))
                .expect("Failed to recover");

            assert_eq!(recovered_items, 2);
            assert_eq!(cache.strategy().disk_limits.current_byte_count, 6);
            assert_eq!(cache.strategy().disk_limits.current_entry_count, 2);
        }
    }
}
