mod memory;
mod noop;

pub use memory::Memory;

#[cfg(test)]
pub(crate) use noop::Noop;
