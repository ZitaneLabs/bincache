mod disk;
mod hybrid;
mod memory;
mod noop;

pub use disk::Disk;
pub use hybrid::Hybrid;
pub use memory::Memory;

#[cfg(test)]
pub(crate) use noop::Noop;
