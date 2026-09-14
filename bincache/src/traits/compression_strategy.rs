use std::borrow::Cow;

use crate::Result;
use async_trait::async_trait;

/// Encode/decode complete cached payloads, preserving their original bytes.
///
/// Implement with `#[async_trait::async_trait]` and provide `Debug`. A codec used
/// by [`crate::Cache`] must also be `Send + Sync`. Returning a borrowed `Cow`
/// avoids allocation when no transformation is needed. `Option<C>` implements
/// this trait: `None` passes bytes through unchanged.
///
/// The cache stores no codec identifier or settings. Reopening persistent data
/// requires a compatible decoder. CPU work executes in the calling future and
/// output buffers have no library-imposed decompression size limit.
///
/// # Examples
///
/// A custom adapter that skips gzip for small values, tagging the stored bytes
/// so either representation can be decoded. Enable `comp_gzip` and add
/// `async-trait = "0.1"`. The tag is this adapter's format, not a bincache header.
///
/// ```
/// # #[cfg(feature = "comp_gzip")]
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use std::borrow::Cow;
/// use async_trait::async_trait;
/// use bincache::{CompressionStrategy, Error, MemoryCacheBuilder, compression::Gzip};
/// use bincache::error::Result;
///
/// #[derive(Debug, Default)]
/// struct ThresholdGzip(Gzip);
///
/// #[async_trait]
/// impl CompressionStrategy for ThresholdGzip {
///     async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
///         let use_gzip = data.len() >= 1024;
///         let body = if use_gzip { self.0.compress(data).await? } else { data };
///         let mut encoded = Vec::with_capacity(1 + body.len());
///         encoded.push(u8::from(use_gzip));
///         encoded.extend_from_slice(&body);
///         Ok(encoded.into())
///     }
///
///     async fn decompress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
///         // Own the body so no result can borrow from a local owned buffer.
///         match data.split_first() {
///             Some((0, body)) => Ok(body.to_vec().into()),
///             Some((1, body)) => self.0.decompress(body.to_vec().into()).await,
///             _ => Err(Error::Custom { message: "Invalid codec tag".into() }),
///         }
///     }
/// }
///
/// let mut cache = MemoryCacheBuilder::default()
///     .with_compression(ThresholdGzip::default()).build().await?;
/// for size in [32, 4096] {
///     let value = vec![b'x'; size];
///     cache.put(size, value.clone()).await?;
///     assert_eq!(cache.get(size).await?.as_ref(), value);
/// }
/// assert!(ThresholdGzip::default().decompress(Cow::Borrowed(&[2])).await.is_err());
/// # Ok(())
/// # }
/// # #[cfg(not(feature = "comp_gzip"))]
/// # fn main() {}
/// ```
#[async_trait]
pub trait CompressionStrategy: std::fmt::Debug {
    /// Encode bytes before storage and capacity checks.
    ///
    /// # Errors
    /// Return encoding errors. Built-in codecs use [`crate::Error::IoError`].
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>>;
    /// Decode stored bytes to exactly the original input.
    ///
    /// # Errors
    /// Return decoding errors for invalid/unsupported input. Built-in codecs
    /// propagate I/O errors, but not every corruption is necessarily detectable.
    async fn decompress<'a>(&self, value: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>>;
}

#[async_trait]
impl<T: CompressionStrategy + Sync + Send> CompressionStrategy for Option<T> {
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        match self {
            Some(compressor) => compressor.compress(data).await,
            None => Ok(data),
        }
    }

    async fn decompress<'a>(&self, value: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        match self {
            Some(compressor) => compressor.decompress(value).await,
            None => Ok(value),
        }
    }
}
