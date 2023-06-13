mod compression_level;
mod maybe_compressor;
mod noop_compressor;

pub use compression_level::CompressionLevel;
pub use maybe_compressor::MaybeCompressor;
pub use noop_compressor::Noop;

// zstd compression

#[cfg(feature = "comp_zstd")]
mod zstd_compressor;

#[cfg(feature = "comp_zstd")]
pub use zstd_compressor::Zstd;
