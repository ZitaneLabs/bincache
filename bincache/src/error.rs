use std::borrow::Cow;

/// An error type used throughout the library.
///
/// Do not match on this type directly, as new variants may be added in the future.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Key not found in cache.")]
    KeyNotFound,

    #[error("Cache limit exceeded: {limit_kind}")]
    LimitExceeded { limit_kind: Cow<'static, str> },

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

pub type Result<T> = std::result::Result<T, Error>;
