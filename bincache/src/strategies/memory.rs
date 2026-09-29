use super::Limits;
use async_trait::async_trait;
use std::borrow::Cow;

use crate::{CacheCapacity, CacheKey, CacheStrategy, Result};

const LIMIT_KIND_BYTE: &str = "Stored bytes";
const LIMIT_KIND_ENTRY: &str = "Stored entries";

#[derive(Debug)]
pub struct Entry {
    data: Vec<u8>,
}

/// Memory-based cache strategy.
///
/// This strategy stores entries in memory. It can be configured to limit the
/// number of bytes and/or entries that can be stored.
#[derive(Default, Debug)]
pub struct Memory {
    limits: Limits,
}

impl Memory {
    /// Create a new memory cache strategy.
    pub fn new(byte_limit: Option<usize>, entry_limit: Option<usize>) -> Self {
        Self {
            limits: Limits::new(byte_limit, entry_limit),
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

        self.limits
            .check(byte_len, None, [LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY])?;

        self.limits.add(byte_len);

        Ok(Entry {
            data: value.into_owned(),
        })
    }

    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(entry.data.as_slice().into())
    }

    async fn replace<'a, K, V>(
        &mut self,
        _key: &K,
        entry: &mut Self::CacheEntry,
        value: V,
    ) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.len();
        let new_total = self.limits.replacement_size(byte_len, entry.data.len())?;

        entry.data = value.into_owned();
        self.limits.current_byte_count = new_total;
        Ok(())
    }

    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        self.limits.remove(entry.data.len());

        Ok(entry.data)
    }

    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        Ok(_ = self.take(entry).await?)
    }

    fn get_cache_capacity(&self) -> Option<CacheCapacity> {
        self.limits.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::{LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY, Memory};
    #[cfg(feature = "comp_gzip")]
    use crate::Cache;
    use crate::async_test;

    use crate::strategies::test_helpers;

    async_test! {
        async fn test_default_strategy() {
            test_helpers::basic(Memory::default(), |strategy| (strategy.limits.current_byte_count, strategy.limits.current_entry_count)).await;
        }

        async fn test_strategy_with_byte_limit() {
            test_helpers::limit(Memory::new(Some(6), None), LIMIT_KIND_BYTE).await;
        }

        async fn test_strategy_with_entry_limit() {
            test_helpers::limit(Memory::new(None, Some(2)), LIMIT_KIND_ENTRY).await;
        }

        async fn test_replace_accounting_capacity_and_failure() {
            test_helpers::replacement(Memory::new(Some(100), None), |strategy| (strategy.limits.current_byte_count, strategy.limits.current_entry_count)).await;
        }

    }

    #[cfg(feature = "comp_gzip")]
    async_test! {
        async fn test_replace_with_compression() {
            let mut cache = Cache::new(Memory::default(), Some(crate::compression::Gzip::default())).await.unwrap();
            cache.put("foo", vec![1; 100]).await.unwrap();
            cache.put("foo", vec![2; 50]).await.unwrap();
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().limits.current_entry_count, 1);
        }
    }
}
