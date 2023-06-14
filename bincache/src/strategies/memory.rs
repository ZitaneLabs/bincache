use async_trait::async_trait;
use std::borrow::Cow;

use crate::{CacheKey, CacheStrategy, Result};

const LIMIT_KIND_BYTE: &str = "Stored bytes";
const LIMIT_KIND_ENTRY: &str = "Stored entries";

#[derive(Debug)]
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

#[async_trait]
impl CacheStrategy for Memory {
    type CacheEntry = Entry;

    async fn put<'a, K, V>(&mut self, _key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.as_ref().len();

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
            data: value.into_owned(),
            byte_len,
        })
    }

    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(entry.data.as_slice().into())
    }

    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        // Decrement limits
        self.current_byte_count -= entry.byte_len;
        self.current_entry_count -= 1;

        Ok(entry.data)
    }

    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        Ok(_ = self.take(entry).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Memory, LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY};
    use crate::{async_test, Cache, Error, NO_COMPRESSION};

    async_test! {
        async fn test_default_strategy() {
            let mut cache = Cache::new(Memory::default(), NO_COMPRESSION);

            cache.put("foo", b"foo".to_vec()).await.unwrap();

            assert_eq!(cache.strategy().current_byte_count, 3);
            assert_eq!(cache.strategy().current_entry_count, 1);

            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.strategy().current_byte_count, 6);
            assert_eq!(cache.strategy().current_entry_count, 2);

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            assert!(cache.get("baz").await.is_err());

            cache.delete("foo").await.unwrap();

            assert_eq!(cache.strategy().current_byte_count, 3);
            assert_eq!(cache.strategy().current_entry_count, 1);

            cache.delete("bar").await.unwrap();

            assert_eq!(cache.strategy().current_byte_count, 0);
            assert_eq!(cache.strategy().current_entry_count, 0);
        }

        async fn test_strategy_with_byte_limit() {
            let mut cache = Cache::new(Memory::new(Some(6), None), NO_COMPRESSION);

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            if let Err(err) = cache.put("baz", b"baz".to_vec()).await {
                match err {
                    Error::LimitExceeded { limit_kind } => {
                        assert_eq!(limit_kind, LIMIT_KIND_BYTE);
                    }
                    _ => panic!("Unexpected error: {:?}", err),
                }
            }
        }

        async fn test_strategy_with_entry_limit() {
            let mut cache = Cache::new(Memory::new(None, Some(3)), NO_COMPRESSION);

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            if let Err(err) = cache.put("baz", b"baz".to_vec()).await {
                match err {
                    Error::LimitExceeded { limit_kind } => {
                        assert_eq!(limit_kind, LIMIT_KIND_ENTRY);
                    }
                    _ => panic!("Unexpected error: {:?}", err),
                }
            }
        }
    }
}
