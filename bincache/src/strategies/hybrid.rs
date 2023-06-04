use std::borrow::Cow;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fs::File, io::Write};

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

    fn put(&mut self, key: &impl CacheKey, value: Vec<u8>) -> Result<Self::CacheEntry> {
        let byte_len = value.len();

        // Evaluate limits
        let fits_into_memory = self.memory_limits.evaluate(byte_len);
        let fits_into_disk = self.disk_limits.evaluate(byte_len);

        // Try to store in memory
        if fits_into_memory.is_satisfied() {
            // Increment limits
            self.memory_limits.current_byte_count += byte_len;
            self.memory_limits.current_entry_count += 1;

            Ok(Entry::Memory(MemoryEntry {
                data: value,
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

    fn get<'a>(&mut self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
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
