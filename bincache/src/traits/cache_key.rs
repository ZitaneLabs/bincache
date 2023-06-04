/// A cache key.
///
/// Keys should be unique and deterministic.
/// The same key should always return the same value.
pub trait CacheKey {
    fn to_key(&self) -> String;
}

// Blanket implementation for all types that implement `ToString`
impl<T> CacheKey for T
where
    T: ToString + ?Sized,
{
    fn to_key(&self) -> String {
        self.to_string()
    }
}
