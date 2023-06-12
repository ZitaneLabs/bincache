use crate::traits::CompressionStrategy;
use crate::Result;
use async_trait::async_trait;
use std::borrow::Cow;

#[derive(Default, Debug)]
pub struct Noop;

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
            let zstd = Noop;
            let compressed = zstd.compress(data.clone().into()).await.unwrap();
            let decompressed = zstd.decompress(compressed).await.unwrap();
            assert_eq!(data.as_slice(), decompressed.as_ref());
        }
    }
}
