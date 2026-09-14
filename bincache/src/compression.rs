mod compression_level;

pub use compression_level::CompressionLevel;

/// A no-op compression strategy.
pub const NO_COMPRESSION: Option<crate::noop::Noop> = None;

#[cfg(feature = "comp_zstd")]
mod zstd_compressor;
#[cfg(feature = "comp_zstd")]
pub use zstd_compressor::Zstd;

#[cfg(feature = "comp_brotli")]
mod brotli_compressor;
#[cfg(feature = "comp_brotli")]
pub use brotli_compressor::Brotli;

#[cfg(feature = "comp_gzip")]
mod gzip_compressor;
#[cfg(feature = "comp_gzip")]
pub use gzip_compressor::Gzip;

#[cfg(test)]
mod tests {
    use crate::{CompressionStrategy, async_test};

    async_test! {
        async fn complete_large_payloads() {
            let codecs: &[&(dyn CompressionStrategy + Send + Sync)] = &[
                &crate::Noop,
                #[cfg(feature = "comp_gzip")]
                &super::Gzip::default(),
                #[cfg(feature = "comp_brotli")]
                &super::Brotli::default(),
                #[cfg(feature = "comp_zstd")]
                &super::Zstd::default(),
            ];
            let record = b"{\"id\":42,\"status\":\"ready\",\"tags\":[\"cache\",\"binary\"]}\n";
            for size in [64 * 1024, 1024 * 1024] {
                let data: Vec<u8> = record.iter().copied().cycle().take(size).collect();
                for codec in codecs {
                    let encoded = codec.compress(data.as_slice().into()).await.unwrap();
                    let decoded = codec.decompress(encoded).await.unwrap();
                    assert!(decoded.as_ref() == data,
                        "{codec:?}: expected {size} original bytes, got {} decoded bytes", decoded.len());
                }
            }
        }
    }
}
