use std::borrow::Cow;

use crate::{
    traits::{CacheKey, CacheStrategy},
    Result,
};

#[derive(Debug)]
pub struct Noop;

impl CacheStrategy for Noop {
    type CacheEntry = ();

    fn put(&mut self, _key: &impl CacheKey, _value: Vec<u8>) -> Result<Self::CacheEntry> {
        Ok(())
    }

    fn get<'a>(&mut self, _entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(Cow::Borrowed(&[]))
    }

    fn take(&mut self, _entry: Self::CacheEntry) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    fn delete(&mut self, _entry: Self::CacheEntry) -> Result<()> {
        Ok(())
    }
}

impl Default for Noop {
    fn default() -> Self {
        Self
    }
}
