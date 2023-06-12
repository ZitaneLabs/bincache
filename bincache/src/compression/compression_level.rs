/// Compression level variants
#[derive(Debug, Clone, Copy)]
pub enum CompressionLevel {
    /// Best compression level for the given compression algorithm
    Best,
    /// Default compression level for the given compression algorithm
    Default,
    /// Fastest compression level for the given compression algorithm
    Fastest,
    /// Specify a custom compression level, which will be clamped to the values
    /// accepted by the underlying compression library.
    Precise(i32),
}

impl From<CompressionLevel> for async_compression::Level {
    fn from(val: CompressionLevel) -> Self {
        use CompressionLevel::*;
        match val {
            Best => async_compression::Level::Best,
            Default => async_compression::Level::Default,
            Fastest => async_compression::Level::Fastest,
            Precise(level) => async_compression::Level::Precise(level),
        }
    }
}
