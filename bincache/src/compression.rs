mod compression_level;
mod maybe_compressor;
mod noop_compressor;
#[cfg(feature = "comp_zstd")]
mod zstd_compressor;

pub use compression_level::CompressionLevel;
pub(crate) use maybe_compressor::MaybeCompressor;
pub(crate) use noop_compressor::Noop;
#[cfg(feature = "comp_zstd")]
pub use zstd_compressor::Zstd;
