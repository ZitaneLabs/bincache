use super::compression_level::CompressionLevel;
use crate::traits::CompressionStrategy;
use crate::Result;
use async_trait::async_trait;
use std::borrow::Cow;

/// A Compressor using Zstd
#[derive(Debug)]
pub struct Zstd {
    level: CompressionLevel,
}

impl Zstd {
    /// Creates a new Zstd Compressor with the given compression level
    pub fn new(level: CompressionLevel) -> Self {
        Zstd { level }
    }
}

impl Default for Zstd {
    /// Creates a new Zstd Compressor with the default compression level
    fn default() -> Self {
        Zstd {
            level: CompressionLevel::Default,
        }
    }
}

#[async_trait]
impl CompressionStrategy for Zstd {
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        #[cfg(feature = "rt_tokio_1")]
        {
            use async_compression::tokio::write;
            use tokio::io::AsyncWriteExt;
            let mut encoder =
                write::ZstdEncoder::with_quality(Vec::with_capacity(data.len()), self.level.into());
            encoder.write_all(data.as_ref()).await?;
            encoder.shutdown().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(any(feature = "blocking", feature = "implicit-blocking"))]
        {
            use async_compression::futures::write;
            use futures_util::AsyncWriteExt;
            let mut encoder =
                write::ZstdEncoder::with_quality(Vec::with_capacity(data.len()), self.level.into());
            encoder.write_all(data.as_ref()).await?;
            encoder.close().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(feature = "rt_async-std_1")]
        {
            use async_compression::futures::write;
            use async_std::io::WriteExt;
            let mut encoder =
                write::ZstdEncoder::with_quality(Vec::with_capacity(data.len()), self.level.into());
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
            let mut encoder = write::ZstdDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.shutdown().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(any(feature = "blocking", feature = "implicit-blocking"))]
        {
            use async_compression::futures::write;
            use futures_util::AsyncWriteExt;
            let mut encoder = write::ZstdDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.close().await?;
            return Ok(encoder.into_inner().into());
        }
        #[cfg(feature = "rt_async-std_1")]
        {
            use async_compression::futures::write;
            use async_std::io::WriteExt;
            let mut encoder = write::ZstdDecoder::new(Vec::with_capacity(data.len()));
            encoder.write_all(data.as_ref()).await?;
            encoder.flush().await?;
            return Ok(encoder.into_inner().into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Zstd;
    use crate::{async_test, traits::CompressionStrategy};

    fn create_arb_data(range: usize) -> Vec<u8> {
        let mut vec = Vec::with_capacity(range);
        for i in 0..range {
            vec.push((i % 255) as u8);
        }
        vec
    }

    async_test! {
        async fn test_compression() {
            let data = create_arb_data(1024);
            let zstd = Zstd::default();
            let compressed = zstd.compress(data.clone().into()).await.unwrap();
            let decompressed = zstd.decompress(compressed).await.unwrap();
            assert_eq!(data.as_slice(), decompressed.as_ref());
        }
    }
}
