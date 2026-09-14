/// A cache key.
///
/// Keys must produce deterministic strings, unique for unequal keys. Disk
/// strategies hash this string into a filename and persist it for recovery.
/// Distinct keys with the same string can overwrite the same disk file even
/// though the in-memory index treats them as different keys.
///
/// Every `ToString` type has a blanket implementation, including strings and
/// integers. Recovery callbacks must map the stored string back to the same key.
pub trait CacheKey {
    /// Return the stable, unique string used by persistent strategies.
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
