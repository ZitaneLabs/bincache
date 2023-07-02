pub struct CacheCapacity {
    total_bytes: usize,
    used_bytes: usize,
}

impl CacheCapacity {
    /// Create a new [CacheCapacity] instance.
    pub fn new(total_bytes: usize, used_bytes: usize) -> Self {
        Self {
            total_bytes,
            used_bytes,
        }
    }

    /// Get the total cache capacity in bytes.
    pub fn total(&self) -> usize {
        self.total_bytes
    }

    /// Get the used cache capacity in bytes.
    pub fn used(&self) -> usize {
        self.used_bytes
    }

    /// Get the cache utilization as a value between 0 and 1.
    pub fn utilization(&self) -> f64 {
        self.used_bytes as f64 / self.total_bytes as f64
    }

    /// Get the cache utilization as a value between 0% and 100%.
    pub fn utilization_percentage(&self) -> f64 {
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.00
    }
}
