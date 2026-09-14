//! Typed builders for choosing storage and optional compression before setup.

use std::hash::Hash;

use crate::{Cache, CacheKey, CacheStrategy, CompressionStrategy, Result, noop::Noop};

/// A builder for creating a new [Cache].
///
/// # Examples
/// ```
/// use bincache::{CacheBuilder, MemoryStrategy};
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() {
///     let mut cache = CacheBuilder::default()
///         .with_strategy(MemoryStrategy::default())
///         .build().await
///         .unwrap();
///
///     cache.put("key", b"value".to_vec()).await.unwrap();
/// }
/// ```
#[derive(Debug, Default)]
pub struct CacheBuilder;

/// Builder with storage selected and compression disabled.
///
/// `Default` uses the strategy's defaults; disk/hybrid default to `./cache`.
/// Build directly or select compression with [`Self::with_compression`].
pub struct CacheBuilderWithStrategy<S> {
    strategy: S,
}

impl<S> Default for CacheBuilderWithStrategy<S>
where
    S: Default + CacheStrategy,
{
    fn default() -> Self {
        CacheBuilderWithStrategy {
            strategy: S::default(),
        }
    }
}

/// Builder with a compressor selected; choose storage with [`Self::with_strategy`].
pub struct CacheBuilderWithCompression<C> {
    compressor: C,
}

impl<C> Default for CacheBuilderWithCompression<C>
where
    C: Default + CompressionStrategy,
{
    fn default() -> Self {
        CacheBuilderWithCompression {
            compressor: C::default(),
        }
    }
}

/// Builder with both storage and compression selected, ready for setup.
pub struct CacheBuilderWithCompressionAndStrategy<S, C> {
    strategy: S,
    compressor: C,
}

impl<S, C> Default for CacheBuilderWithCompressionAndStrategy<S, C>
where
    S: Default + CacheStrategy,
    C: Default + CompressionStrategy,
{
    fn default() -> Self {
        CacheBuilderWithCompressionAndStrategy {
            strategy: S::default(),
            compressor: C::default(),
        }
    }
}

impl CacheBuilder {
    /// Select storage; setup is deferred until `build().await`.
    pub fn with_strategy<S>(self, strategy: S) -> CacheBuilderWithStrategy<S>
    where
        S: CacheStrategy,
    {
        CacheBuilderWithStrategy { strategy }
    }

    /// Select the codec applied before writes and after reads/takes.
    pub fn with_compression<C>(self, compressor: C) -> CacheBuilderWithCompression<C>
    where
        C: CompressionStrategy,
    {
        CacheBuilderWithCompression { compressor }
    }
}

impl<C> CacheBuilderWithCompression<C> {
    /// Select storage; setup is deferred until `build().await`.
    pub fn with_strategy<S>(self, strategy: S) -> CacheBuilderWithCompressionAndStrategy<S, C> {
        {
            CacheBuilderWithCompressionAndStrategy {
                strategy,
                compressor: self.compressor,
            }
        }
    }
}

impl<S> CacheBuilderWithStrategy<S>
where
    S: CacheStrategy + Send,
{
    /// Select the codec applied before writes and after reads/takes.
    pub fn with_compression<C>(self, compressor: C) -> CacheBuilderWithCompressionAndStrategy<S, C>
    where
        C: CompressionStrategy,
    {
        CacheBuilderWithCompressionAndStrategy {
            strategy: self.strategy,
            compressor,
        }
    }

    /// Set up storage and create an empty cache without compression.
    ///
    /// The key type is inferred from later operations or selected with `build::<K>()`.
    /// Existing disk data requires an explicit [`Cache::recover`] call.
    ///
    /// # Errors
    /// Returns strategy setup errors, including directory creation failures.
    pub async fn build<K>(self) -> Result<Cache<K, S, Noop>>
    where
        K: CacheKey + Eq + Hash + Sync + Send,
    {
        Cache::new(self.strategy, None).await
    }
}

impl<S, C> CacheBuilderWithCompressionAndStrategy<S, C>
where
    S: CacheStrategy + Send,
    C: CompressionStrategy,
{
    /// Set up storage and create an empty cache using the selected compressor.
    ///
    /// Existing disk data requires an explicit [`Cache::recover`] call with the
    /// same codec and key mapping used when writing it.
    ///
    /// # Errors
    /// Returns strategy setup errors, including directory creation failures.
    pub async fn build<K>(self) -> Result<Cache<K, S, C>>
    where
        K: CacheKey + Eq + Hash + Sync + Send,
        C: CompressionStrategy + Sync + Send,
    {
        Cache::new(self.strategy, Some(self.compressor)).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{async_test, noop::Noop};

    use super::*;

    async_test! {
        async fn test_default() {
            _ = CacheBuilder;
        }

        async fn test_type_aliased() {
            type NoopCacheBuilder = CacheBuilderWithStrategy<Noop>;
            _ = NoopCacheBuilder::default().build::<String>();
        }

        async fn test_key_inference() {
            let mut cache = CacheBuilder.with_strategy(Noop).build().await.unwrap();
            cache.put("test".to_string(), vec![]).await.unwrap();
        }
    }
}
