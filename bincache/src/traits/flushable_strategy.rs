use super::{CacheKey, CacheStrategy};
use crate::Result;

/// A cache strategy that can flush its data to a non-volatile storage.
pub trait FlushableStrategy: CacheStrategy {
    fn flush(
        &mut self,
        key: &impl CacheKey,
        entry: &Self::CacheEntry,
    ) -> Result<Option<Self::CacheEntry>>;
}
