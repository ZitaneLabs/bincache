use std::borrow::Cow;

use crate::{
    traits::{CacheKey, CacheStrategy},
    Result,
};

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
    pub fn new(byte_limit: Option<usize>, entry_limit: Option<usize>) -> Self {
        Self {
            byte_limit,
            entry_limit,
            ..Default::default()
        }
    }
}

impl CacheStrategy for Memory {
    type CacheEntry = Entry;

    fn put(&mut self, _key: &impl CacheKey, value: Vec<u8>) -> Result<Self::CacheEntry> {
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

    fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(entry.data.as_slice().into())
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

#[cfg(test)]
mod tests {
    use super::{Memory, LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY};
    use crate::{Cache, Error};

    #[test]
    fn test_default_strategy() {
        let mut cache = Cache::new(Memory::default());

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
        let mut cache = Cache::new(Memory::new(Some(6), None));

        cache.put("foo", b"foo".to_vec()).unwrap();
        cache.put("bar", b"bar".to_vec()).unwrap();

        assert_eq!(cache.get("foo").unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").unwrap(), b"bar".as_slice());

        match cache.put("baz", b"baz".to_vec()) {
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
        let mut cache = Cache::new(Memory::new(None, Some(3)));

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
