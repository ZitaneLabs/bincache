use crate::{traits::CacheStrategy, Result};

#[derive(Debug)]
pub struct Noop;

impl CacheStrategy for Noop {
    type CacheEntry = ();

    fn put(&mut self, _value: Vec<u8>) -> Result<Self::CacheEntry> {
        Ok(())
    }

    fn get<'a>(&mut self, _entry: &'a Self::CacheEntry) -> Result<&'a [u8]> {
        Ok(&[])
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
