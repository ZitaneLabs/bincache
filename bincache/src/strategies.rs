mod disk;
mod memory;
mod noop;

pub use disk::Disk;
pub use memory::Memory;

#[cfg(test)]
pub(crate) use noop::Noop;
