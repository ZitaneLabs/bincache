use std::hash::Hash;

use super::Error;

use crate::cache::Cache;
use crate::traits::{CacheKey, CacheStrategy};
use crate::Result;

/// A builder for creating a new [Cache].
///
/// # Examples
/// ```
/// use bincache::{CacheBuilder, MemoryStrategy};
///
/// let mut cache = CacheBuilder::default()
///     .with_strategy(MemoryStrategy::default())
///     .build()
///     .unwrap();
///
/// cache.put("key", b"value".to_vec()).unwrap();
/// ```
#[derive(Debug)]
pub struct CacheBuilder<S> {
    strategy: Option<S>,
}

impl<S> CacheBuilder<S> {
    /// Set the [CacheStrategy].
    pub fn with_strategy<_S>(self, strategy: _S) -> CacheBuilder<_S>
    where
        _S: CacheStrategy,
    {
        CacheBuilder {
            strategy: Some(strategy),
        }
    }
}

impl<S> CacheBuilder<S>
where
    S: CacheStrategy + Default,
{
    pub fn new() -> CacheBuilder<S> {
        CacheBuilder {
            strategy: Some(S::default()),
        }
    }
}

impl<S> CacheBuilder<S>
where
    S: CacheStrategy,
{
    /// Build the [Cache].
    pub fn build<K>(self) -> Result<Cache<K, S>>
    where
        K: CacheKey + Eq + Hash,
    {
        Ok(Cache::new(self.strategy.ok_or(Error::NoStrategy)?))
    }
}

impl Default for CacheBuilder<()> {
    /// Create a new [CacheBuilder] with the default configuration.
    fn default() -> CacheBuilder<()> {
        CacheBuilder { strategy: None }
    }
}

#[cfg(test)]
mod tests {
    use crate::strategies::Noop;

    use super::*;

    #[test]
    fn test_default() {
        _ = CacheBuilder::default();
    }

    #[test]
    fn test_type_aliased() {
        type NoopCacheBuilder = CacheBuilder<Noop>;
        _ = NoopCacheBuilder::new().build::<String>().unwrap();
    }

    #[test]
    fn test_key_inference() {
        let mut cache = CacheBuilder::default().with_strategy(Noop).build().unwrap();
        cache.put("test".to_string(), vec![]).unwrap();
    }
}
