mod compression_level;

pub use compression_level::CompressionLevel;

/// A no-op compression strategy.
pub const NO_COMPRESSION: Option<crate::noop::Noop> = None;

// zstd

#[cfg(feature = "comp_zstd")]
mod zstd_compressor;

#[cfg(feature = "comp_zstd")]
pub use zstd_compressor::Zstd;
