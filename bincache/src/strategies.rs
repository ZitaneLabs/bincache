mod disk;
mod hybrid;
mod memory;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod test_helpers;

pub use disk::Disk;
pub use hybrid::Hybrid;
pub use memory::Memory;

use crate::{CacheCapacity, Error, Result};

/// Byte and entry limits with their current usage.
#[derive(Debug, Default)]
pub struct Limits {
    byte_limit: Option<usize>,
    entry_limit: Option<usize>,
    current_byte_count: usize,
    current_entry_count: usize,
}

impl Limits {
    pub fn new(byte_limit: Option<usize>, entry_limit: Option<usize>) -> Self {
        Self {
            byte_limit,
            entry_limit,
            ..Default::default()
        }
    }

    fn check(&self, size: usize, old_size: Option<usize>, kinds: [&'static str; 2]) -> Result<()> {
        let bytes = self.current_byte_count - old_size.unwrap_or(0) + size;
        let entries = self.current_entry_count - usize::from(old_size.is_some()) + 1;
        let limit_kind = if self.byte_limit.is_some_and(|limit| bytes > limit) {
            kinds[0]
        } else if self.entry_limit.is_some_and(|limit| entries > limit) {
            kinds[1]
        } else {
            return Ok(());
        };
        Err(Error::LimitExceeded {
            limit_kind: limit_kind.into(),
        })
    }

    fn add(&mut self, size: usize) {
        self.current_byte_count += size;
        self.current_entry_count += 1;
    }

    fn remove(&mut self, size: usize) {
        self.current_byte_count -= size;
        self.current_entry_count -= 1;
    }

    fn capacity(&self) -> Option<CacheCapacity> {
        self.byte_limit
            .map(|limit| CacheCapacity::new(limit, self.current_byte_count))
    }
}
