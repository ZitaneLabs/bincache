use std::borrow::Cow;

use crate::Result;
use async_trait::async_trait;

/// A compression strategy.
#[async_trait]
pub trait CompressionStrategy: std::fmt::Debug {
    /// Compress binary data
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>>;
    /// Decompress binary data
    async fn decompress<'a>(&self, value: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>>;
}
