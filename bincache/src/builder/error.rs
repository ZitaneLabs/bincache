#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No strategy was provided")]
    NoStrategy,
}
