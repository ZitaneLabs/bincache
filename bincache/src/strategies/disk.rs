use std::borrow::Cow;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fs::File, io::Write};

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
    pub fn new(byte_limit: Option<usize>, entry_limit: Option<usize>) -> Self {
        Self {
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
