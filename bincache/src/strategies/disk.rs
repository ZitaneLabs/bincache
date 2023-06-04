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

const LIMIT_KIND_BYTE: &str = "Stored bytes";
const LIMIT_KIND_ENTRY: &str = "Stored entries";

pub struct Entry {
    path: PathBuf,
    byte_len: usize,
}

/// Disk-based cache strategy.
///
/// This strategy stores entries on disk. It can be configured to limit the
/// number of bytes and/or entries that can be stored.
#[derive(Default, Debug)]
pub struct Disk {
    /// The directory where entries are stored.
    cache_dir: PathBuf,
    /// The maximum number of bytes that can be stored.
    byte_limit: Option<usize>,
    /// The maximum number of entries that can be stored.
    entry_limit: Option<usize>,
    /// The current number of bytes stored.
    current_byte_count: usize,
    /// The current number of entries stored.
    current_entry_count: usize,
}

impl Disk {
    /// Create a new disk cache strategy.
    pub fn new<'a>(
        cache_dir: impl Into<Cow<'a, Path>>,
        byte_limit: Option<usize>,
        entry_limit: Option<usize>,
    ) -> Self {
        Self {
            cache_dir: cache_dir.into().into_owned(),
            byte_limit,
            entry_limit,
            ..Default::default()
        }
    }
}

impl Disk {
    fn read_from_disk(&self, entry: &Entry) -> Result<Vec<u8>> {
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

    fn delete_from_disk(&self, entry: &Entry) -> Result<()> {
        Ok(std::fs::remove_file(&entry.path)?)
    }
}

impl CacheStrategy for Disk {
    type CacheEntry = Entry;

    fn put(&mut self, key: &impl CacheKey, value: Vec<u8>) -> Result<Self::CacheEntry> {
        let byte_len = value.len();

        // Check if the byte limit has been reached.
        if let Some(byte_limit) = self.byte_limit {
            if self.current_byte_count + byte_len > byte_limit {
                return Err(crate::Error::LimitExceeded {
                    limit_kind: LIMIT_KIND_BYTE.into(),
                });
            }
        }

        // Check if entry limit has been reached.
        if let Some(entry_limit) = self.entry_limit {
            if self.current_entry_count + 1 > entry_limit {
                return Err(crate::Error::LimitExceeded {
                    limit_kind: LIMIT_KIND_ENTRY.into(),
                });
            }
        }

        // Write to disk
        let path = self.cache_dir.join(key.to_key());
        self.write_to_disk(&path, value.as_slice())?;

        // Increment limits
        self.current_byte_count += byte_len;
        self.current_entry_count += 1;

        Ok(Entry { path, byte_len })
    }

    fn get<'a>(&mut self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        self.read_from_disk(entry).map(Cow::Owned)
    }

    fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        let data = self.read_from_disk(&entry)?;
        self.delete(entry)?;

        Ok(data)
    }

    fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        self.delete_from_disk(&entry)?;

        // Decrement limits
        self.current_byte_count -= entry.byte_len;
        self.current_entry_count -= 1;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Disk, LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY};
    use crate::{test_utils::TempDir, Cache, Error};

    #[test]
    fn test_default() {
        let temp_dir = TempDir::new();
        let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None));

        cache.put("foo", b"foo".to_vec()).unwrap();

        assert_eq!(cache.strategy().current_byte_count, 3);
        assert_eq!(cache.strategy().current_entry_count, 1);

        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.strategy().current_byte_count, 6);
        assert_eq!(cache.strategy().current_entry_count, 2);

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        assert!(cache.get("baz").is_err());

        cache.delete("foo").unwrap();

        assert_eq!(cache.strategy().current_byte_count, 3);
        assert_eq!(cache.strategy().current_entry_count, 1);

        cache.delete("bar").unwrap();

        assert_eq!(cache.strategy().current_byte_count, 0);
        assert_eq!(cache.strategy().current_entry_count, 0);
    }

    #[test]
    fn test_strategy_with_byte_limit() {
        let temp_dir = TempDir::new();
        let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), Some(6), None));

        let foo_data = b"foo".to_vec();
        let bar_data = b"bar".to_vec();
        let baz_data = b"baz".to_vec();

        assert_eq!(foo_data.len(), 3);
        assert_eq!(bar_data.len(), 3);
        assert_eq!(baz_data.len(), 3);

        cache.put("foo", foo_data.clone()).unwrap();
        cache.put("bar", bar_data.clone()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), foo_data.as_slice());
        assert_eq!(cache.get("bar").unwrap(), bar_data.as_slice());

        match cache.put("baz", baz_data) {
            Err(err) => match err {
                Error::LimitExceeded { limit_kind } => {
                    assert_eq!(limit_kind, LIMIT_KIND_BYTE);
                }
                _ => panic!("Unexpected error: {:?}", err),
            },
            _ => (),
        }
    }

    #[test]
    fn test_strategy_with_entry_limit() {
        let temp_dir = TempDir::new();
        let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, Some(3)));

        cache.put("foo", b"foo".to_vec()).unwrap();
        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        match cache.put("baz", b"baz".to_vec()) {
            Err(err) => match err {
                Error::LimitExceeded { limit_kind } => {
                    assert_eq!(limit_kind, LIMIT_KIND_ENTRY);
                }
                _ => panic!("Unexpected error: {:?}", err),
            },
            _ => (),
        }
    }
}
