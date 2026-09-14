use async_trait::async_trait;
use std::borrow::Cow;

use crate::{CacheCapacity, Result};

use super::CacheKey;

/// Storage and accounting for entries indexed by [`crate::Cache`].
///
/// The cache owns the key-to-entry map and handles compression. Strategy methods
/// receive stored (already compressed) bytes and must not compress them again.
/// Implement with `#[async_trait::async_trait]`; generated futures are `Send`.
/// The strategy and entry types determine whether a cache is `Send`/`Sync`.
///
/// # Examples
///
/// A custom storage strategy that keeps each payload in an immutable boxed slice.
/// This intentionally has no limits or persistence; the cache supplies the index.
/// Add `async-trait = "0.1"` to the implementing crate.
///
/// ```
/// use std::borrow::Cow;
/// use async_trait::async_trait;
/// use bincache::{Cache, CacheCapacity, CacheKey, CacheStrategy, NO_COMPRESSION};
/// use bincache::error::Result;
///
/// struct BoxedStorage;
///
/// #[async_trait]
/// impl CacheStrategy for BoxedStorage {
///     type CacheEntry = Box<[u8]>;
///
///     async fn put<'a, K, V>(&mut self, _key: &K, value: V) -> Result<Self::CacheEntry>
///     where
///         K: CacheKey + Send + Sync,
///         V: Into<Cow<'a, [u8]>> + Send,
///     {
///         Ok(value.into().into_owned().into_boxed_slice())
///     }
///
///     async fn replace<'a, K, V>(
///         &mut self, key: &K, entry: &mut Self::CacheEntry, value: V,
///     ) -> Result<()>
///     where
///         K: CacheKey + Send + Sync,
///         V: Into<Cow<'a, [u8]>> + Send,
///     {
///         let replacement = self.put(key, value).await?;
///         *entry = replacement;
///         Ok(())
///     }
///
///     async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
///         Ok(Cow::Borrowed(entry))
///     }
///
///     async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
///         Ok(entry.into_vec())
///     }
///
///     async fn delete(&mut self, _entry: Self::CacheEntry) -> Result<()> {
///         Ok(()) // dropping the box frees its storage
///     }
///
///     fn get_cache_capacity(&self) -> Option<CacheCapacity> {
///         None
///     }
/// }
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<()> {
/// let mut cache = Cache::new(BoxedStorage, NO_COMPRESSION).await?;
/// cache.put("response", b"first".to_vec()).await?;
/// cache.put("response", b"updated".to_vec()).await?;
/// assert_eq!(cache.get("response").await?.as_ref(), b"updated");
/// assert_eq!(cache.take("response").await?, b"updated");
/// cache.put("temporary", b"discard".to_vec()).await?;
/// cache.delete("temporary").await?;
/// assert!(!cache.exists("temporary"));
/// # Ok(())
/// # }
/// ```
#[async_trait]
pub trait CacheStrategy {
    /// This type is opaque to the cache.
    /// It is used to store information about each cached data entry.
    type CacheEntry;

    /// Initialize storage before the cache becomes usable; defaults to no work.
    ///
    /// # Errors
    /// Return initialization errors, such as directory creation failures.
    async fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    /// Store a new key's payload and return its entry handle.
    ///
    /// The cache calls this only for a key absent from its index. Update capacity
    /// accounting after successful storage. Values may be borrowed or owned.
    ///
    /// # Errors
    /// Return storage or limit errors; the cache will not index a failed insert.
    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send;

    /// Replace an existing entry without temporarily counting both values.
    ///
    /// If this operation fails, `entry` must remain valid and unchanged.
    /// Account for the new payload minus the old payload, preserving the entry
    /// count. The default rejects replacement without changing anything.
    ///
    /// # Errors
    /// Return storage/capacity errors or an unsupported-operation error.
    async fn replace<'a, K, V>(
        &mut self,
        _key: &K,
        _entry: &mut Self::CacheEntry,
        _value: V,
    ) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        Err(crate::Error::Custom {
            message: "Cache strategy does not support replacing entries".into(),
        })
    }

    /// Read stored bytes, borrowing the entry's buffer when possible.
    ///
    /// The cache decompresses the result. Do not migrate or remove the entry.
    ///
    /// # Errors
    /// Return storage/decoding errors, such as missing or malformed files.
    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>>;

    /// Consume the handle, remove storage and return stored bytes before decompression.
    ///
    /// Release capacity accounting on success. The cache has already removed
    /// the key from its index, so it cannot restore the entry if this fails.
    ///
    /// # Errors
    /// Return storage errors; partial failure is not rolled back by the cache.
    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>>;

    /// Consume the handle, remove storage and release its capacity accounting.
    ///
    /// The key has already been removed from the cache's index.
    ///
    /// # Errors
    /// Return deletion errors; the cache cannot restore a consumed handle.
    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()>;

    /// Report the configured byte limit and current stored payload usage.
    ///
    /// Return `None` if no byte capacity can be reported. Built-in strategies
    /// count compressed payload lengths, excluding metadata; hybrid requires
    /// both tier byte limits. Entry-only limits need not produce a snapshot.
    fn get_cache_capacity(&self) -> Option<CacheCapacity>;
}
