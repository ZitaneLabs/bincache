use async_trait::async_trait;
use std::borrow::Cow;

use crate::{
    traits::{CacheKey, CacheStrategy},
    Result,
};

#[derive(Debug)]
pub struct Noop;

#[async_trait]
impl CacheStrategy for Noop {
    type CacheEntry = ();

    async fn put<'a, K, V>(&mut self, _key: &K, _value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        Ok(())
    }

    async fn get<'a>(&self, _entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(Cow::Borrowed(&[]))
    }

    async fn take(&mut self, _entry: Self::CacheEntry) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn delete(&mut self, _entry: Self::CacheEntry) -> Result<()> {
        Ok(())
    }
}

impl Default for Noop {
    fn default() -> Self {
        Self
    }
}
