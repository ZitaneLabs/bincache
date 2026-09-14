use std::borrow::Cow;

/// An error type used throughout the library.
///
/// Do not match on this type directly, as new variants may be added in the future.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The key is absent from the in-memory index.
    #[error("Key not found in cache.")]
    KeyNotFound,

    /// A configured storage-byte or entry-count limit would be exceeded.
    #[error("Cache limit exceeded: {limit_kind}")]
    LimitExceeded {
        /// Human-readable description of the exceeded dimension/tier.
        limit_kind: Cow<'static, str>,
    },

    /// Filesystem or built-in codec I/O failed; inspect the source for details.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// An error variant for custom implementations.
    ///
    /// Use this to wrap any error type that implements `std::error::Error`.
    #[error("{0}")]
    CustomError(
        /// The custom error.
        #[from]
        Box<dyn std::error::Error + Send + Sync>,
    ),

    /// An error variant for custom implementations.
    ///
    /// Use this to provide a custom error message.
    #[error("{message}")]
    Custom {
        /// The custom error message.
        message: String,
    },
}

/// Result type returned by cache, storage, and compression operations.
pub type Result<T> = std::result::Result<T, Error>;
