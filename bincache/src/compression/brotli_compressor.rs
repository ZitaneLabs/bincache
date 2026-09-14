use super::compression_level::CompressionLevel;
use crate::Result;
use crate::traits::CompressionStrategy;
use async_trait::async_trait;
use std::borrow::Cow;

/// Brotli codec, available with the `comp_brotli` feature.
///
/// Processes complete buffers and allocates output on each call. `Default` uses
/// the underlying codec's default level; CPU work runs in the calling future.
/// See the [compression example](crate#compression).
#[derive(Debug)]
pub struct Brotli {
    level: CompressionLevel,
}

impl Brotli {
    /// Creates a new Brotli Compressor with the given compression level
    pub fn new(level: CompressionLevel) -> Self {
        Self { level }
    }
}

impl Default for Brotli {
    /// Creates a new Brotli Compressor with the default compression level
    fn default() -> Self {
        Self {
            level: CompressionLevel::Default,
        }
    }
}

#[async_trait]
impl CompressionStrategy for Brotli {
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        #[cfg(feature = "rt_tokio_1")]
        {
            use async_compression::tokio::write;
            use tokio::io::AsyncWriteExt;
            let mut encoder = write::BrotliEncoder::with_quality(
                Vec::with_capacity(data.len()),
                self.level.into(),
            );
            encoder.write_all(data.as_ref()).await?;
            encoder.shutdown().await?;
            Ok(encoder.into_inner().into())
        }
        #[cfg(not(feature = "rt_tokio_1"))]
        {
            use async_compression::futures::write;
            use futures_util::AsyncWriteExt;
            let mut encoder = write::BrotliEncoder::with_quality(
                Vec::with_capacity(data.len()),
                self.level.into(),
            );
            encoder.write_all(data.as_ref()).await?;
            encoder.close().await?;
            Ok(encoder.into_inner().into())
        }
    }

    async fn decompress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        #[cfg(feature = "rt_tokio_1")]
        {
            use async_compression::tokio::write;
            use tokio::io::AsyncWriteExt;
            let mut encoder = write::BrotliDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.shutdown().await?;
            Ok(encoder.into_inner().into())
        }
        #[cfg(not(feature = "rt_tokio_1"))]
        {
            use async_compression::futures::write;
            use futures_util::AsyncWriteExt;
            let mut encoder = write::BrotliDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.close().await?;
            Ok(encoder.into_inner().into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Brotli;
    use crate::{async_test, traits::CompressionStrategy, utils::test::create_arb_data};

    async_test! {
        async fn test_compression() {
            let data = create_arb_data(1024);
            let brotli = Brotli::default();
            let compressed = brotli.compress(data.clone().into()).await.unwrap();
            let decompressed = brotli.decompress(compressed).await.unwrap();
            assert_eq!(data.as_slice(), decompressed.as_ref());
        }
    }
}
