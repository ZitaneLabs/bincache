use super::compression_level::CompressionLevel;
use crate::traits::CompressionStrategy;
use crate::Result;
use async_trait::async_trait;
use std::borrow::Cow;

#[derive(Debug)]
pub struct Gzip {
    level: CompressionLevel,
}

impl Gzip {
    /// Creates a new Gzip Compressor with the given compression level
    pub fn new(level: CompressionLevel) -> Self {
        Self { level }
    }
}

impl Default for Gzip {
    /// Creates a new Gzip Compressor with the default compression level
    fn default() -> Self {
        Self {
            level: CompressionLevel::Default,
        }
    }
}

#[async_trait]
impl CompressionStrategy for Gzip {
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        #[cfg(feature = "rt_tokio_1")]
        {
            use async_compression::tokio::write;
            use tokio::io::AsyncWriteExt;
            let mut encoder =
                write::GzipEncoder::with_quality(Vec::with_capacity(data.len()), self.level.into());
            encoder.write_all(data.as_ref()).await?;
            encoder.shutdown().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(any(feature = "blocking", feature = "implicit-blocking"))]
        {
            use async_compression::futures::write;
            use futures_util::AsyncWriteExt;
            let mut encoder =
                write::GzipEncoder::with_quality(Vec::with_capacity(data.len()), self.level.into());
            encoder.write_all(data.as_ref()).await?;
            encoder.close().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(feature = "rt_async-std_1")]
        {
            use async_compression::futures::write;
            use async_std::io::WriteExt;
            let mut encoder =
                write::GzipEncoder::with_quality(Vec::with_capacity(data.len()), self.level.into());
            encoder.write_all(data.as_ref()).await?;
            encoder.flush().await?;
            return Ok(encoder.into_inner().into());
        }
    }

    async fn decompress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        #[cfg(feature = "rt_tokio_1")]
        {
            use async_compression::tokio::write;
            use tokio::io::AsyncWriteExt;
            let mut encoder = write::GzipDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.shutdown().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(any(feature = "blocking", feature = "implicit-blocking"))]
        {
            use async_compression::futures::write;
            use futures_util::AsyncWriteExt;
            let mut encoder = write::GzipDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.close().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(feature = "rt_async-std_1")]
        {
            use async_compression::futures::write;
            use async_std::io::WriteExt;
            let mut encoder = write::GzipDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.flush().await?;
            return Ok(encoder.into_inner().into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Gzip;
    use crate::{async_test, traits::CompressionStrategy, utils::test::create_arb_data};

    async_test! {
        async fn test_compression() {
            let data = create_arb_data(1024);
            let gzip = Gzip::default();
            let compressed = gzip.compress(data.clone().into()).await.unwrap();
            let decompressed = gzip.decompress(compressed).await.unwrap();
            assert_eq!(data.as_slice(), decompressed.as_ref());
        }
    }
}
