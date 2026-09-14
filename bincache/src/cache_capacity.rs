/// A snapshot of configured byte capacity and tracked stored payload usage.
///
/// Excludes keys, allocation/index overhead and disk file headers. With
/// compression enabled, usage counts compressed bytes. This is not a measure
/// of process RAM or filesystem space; see [`crate::Cache::capacity`].
pub struct CacheCapacity {
    total_bytes: usize,
    used_bytes: usize,
}

impl CacheCapacity {
    /// Construct a snapshot without validating or clamping either value.
    ///
    /// `used_bytes` can exceed `total_bytes`, for example after recovery.
    pub fn new(total_bytes: usize, used_bytes: usize) -> Self {
        Self {
            total_bytes,
            used_bytes,
        }
    }

    /// Return the configured byte limit (the sum of tier limits for hybrid).
    pub fn total(&self) -> usize {
        self.total_bytes
    }

    /// Return tracked stored payload bytes, after optional compression.
    pub fn used(&self) -> usize {
        self.used_bytes
    }

    /// Return `used / total`, without clamping.
    ///
    /// Normally between zero and one, but can exceed one after recovery. Zero
    /// total yields `NaN` for zero usage or infinity for nonzero usage.
    pub fn utilization(&self) -> f64 {
        self.used_bytes as f64 / self.total_bytes as f64
    }

    /// Return [`Self::utilization`] multiplied by 100, with the same zero-total
    /// and over-capacity behavior.
    pub fn utilization_percentage(&self) -> f64 {
        self.utilization() * 100.00
    }
}
