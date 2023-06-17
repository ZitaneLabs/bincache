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
