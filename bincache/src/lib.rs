#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(not(any(
    feature = "implicit-blocking",
    feature = "blocking",
    feature = "rt_tokio_1",
    feature = "rt_async-std_1"
)))]
compile_error!(
    "Cannot run without an async runtime.\nPlease enable one of the following features: [blocking, rt_tokio_1, rt_async-std_1]."
);

#[cfg(any(
    all(feature = "blocking", feature = "rt_tokio_1"),
    all(feature = "blocking", feature = "rt_async-std_1"),
    all(feature = "rt_tokio_1", feature = "rt_async-std_1")
))]
compile_error!("Cannot enable multiple async runtime features at the same time.");

mod cache;
mod macros;
mod noop;

pub mod cache_builder;
/// Stored-payload capacity snapshots.
pub mod cache_capacity;
/// Optional codecs and algorithm-specific compression levels.
pub mod compression;
/// Cache, strategy, and codec error types.
pub mod error;
/// Built-in memory, disk, and hybrid storage strategies.
pub mod strategies;
/// Extension traits for keys, storage, recovery, flushing, and compression.
pub mod traits;
/// Internal implementation utilities; no public helpers are currently exposed.
pub mod utils;

pub(crate) use error::Result;
pub(crate) use utils::disk_util as DiskUtil;

// Export basic types
pub use cache::Cache;
pub use cache_builder::CacheBuilder;
pub use cache_capacity::CacheCapacity;
pub use compression::NO_COMPRESSION;
pub use error::Error;
pub use noop::Noop;
pub use traits::*;

// Export typed caches and builders
macros::reexport_strategy!(Disk, DiskCache, DiskCacheBuilder, DiskStrategy);
macros::reexport_strategy!(Hybrid, HybridCache, HybridCacheBuilder, HybridStrategy);
macros::reexport_strategy!(Memory, MemoryCache, MemoryCacheBuilder, MemoryStrategy);
