use crate::Result;
use async_trait::async_trait;
use std::borrow::Cow;

use crate::{compression::Noop, traits::CompressionStrategy};

/// Workaround for optional compression
#[derive(Debug)]
pub enum MaybeCompressor<T>
where
    T: CompressionStrategy + Sync + Send,
{
    Compressor(T),
    Passthrough,
}

#[async_trait]
impl<T> CompressionStrategy for MaybeCompressor<T>
where
    T: CompressionStrategy + Sync + Send,
{
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        match self {
            Self::Compressor(compressor) => compressor.compress(data).await,
            Self::Passthrough => Noop::default().compress(data).await,
        }
    }

    async fn decompress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        match self {
            Self::Compressor(compressor) => compressor.decompress(data).await,
            Self::Passthrough => Noop::default().decompress(data).await,
        }
    }
}

impl MaybeCompressor<Noop> {
    pub fn noop() -> Self {
        MaybeCompressor::<Noop>::Passthrough
    }
}
