use crate::{traits::CacheStrategy, Result};

const LIMIT_KIND_BYTE: &str = "Stored bytes";
const LIMIT_KIND_ENTRY: &str = "Stored entries";

pub struct Entry {
    data: Vec<u8>,
    byte_len: usize,
}

/// Memory-based cache strategy.
///
/// This strategy stores entries in memory. It can be configured to limit the
/// number of bytes and/or entries that can be stored.
#[derive(Default, Debug)]
pub struct Memory {
    /// The maximum number of bytes that can be stored.
    byte_limit: Option<usize>,
    /// The maximum number of entries that can be stored.
    entry_limit: Option<usize>,
    /// The current number of bytes stored.
    current_byte_count: usize,
    /// The current number of entries stored.
    current_entry_count: usize,
}

impl Memory {
    /// Create a new memory cache strategy.
    pub fn new(byte_limit: Option<usize>, entry_limit: Option<usize>) -> Memory {
        Memory {
            byte_limit,
            entry_limit,
            ..Default::default()
        }
    }
}

impl CacheStrategy for Memory {
    type CacheEntry = Entry;

    fn put(&mut self, value: Vec<u8>) -> Result<Self::CacheEntry> {
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

        // Increment limits
        self.current_byte_count += byte_len;
        self.current_entry_count += 1;

        Ok(Entry {
            data: value,
            byte_len,
        })
    }

    fn get<'a>(&mut self, entry: &'a Self::CacheEntry) -> Result<&'a [u8]> {
        Ok(&entry.data[..])
    }

    fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        // Decrement limits
        self.current_byte_count -= entry.byte_len;
        self.current_entry_count -= 1;

        Ok(entry.data)
    }

    fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        Ok(_ = self.take(entry)?)
    }
}
