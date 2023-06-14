use crate::Result;
use crate::{CacheKey, CacheStrategy, CompressionStrategy};
use async_trait::async_trait;
use std::borrow::Cow;

/// A no-op object that implements both `CacheStrategy` and `CompressionStrategy`.
/// Can be used as a placeholder in testing, or as a default compression strategy.
#[derive(Default, Debug)]
pub struct Noop;

#[async_trait]
impl CacheStrategy for Noop {
    type CacheEntry = ();

    async fn put<'a, K, V>(&mut self, _key: &K, _value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        Ok(())
    }

    async fn get<'a>(&self, _entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(Cow::Borrowed(&[]))
    }

    async fn take(&mut self, _entry: Self::CacheEntry) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn delete(&mut self, _entry: Self::CacheEntry) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl CompressionStrategy for Noop {
    async fn compress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        Ok(data)
    }

    async fn decompress<'a>(&self, data: Cow<'a, [u8]>) -> Result<Cow<'a, [u8]>> {
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::Noop;
    use crate::{async_test, CompressionStrategy};

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
            let zstd = Noop;
            let compressed = zstd.compress(data.clone().into()).await.unwrap();
            let decompressed = zstd.decompress(compressed).await.unwrap();
            assert_eq!(data.as_slice(), decompressed.as_ref());
        }
    }
}
