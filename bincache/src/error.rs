use std::borrow::Cow;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    BuildError(#[from] crate::builder::Error),
    #[error("Key not found in cache.")]
    KeyNotFound,
    #[error("Cache limit exceeded: {limit_kind}")]
    LimitExceeded { limit_kind: Cow<'static, str> },
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
