//! # bincache
//!
//! `bincache` is a versatile, high-performance, async-first binary data caching library for Rust, designed with a focus on flexibility, efficiency, and ease of use. It enables developers to store, retrieve, and manage binary data using various caching strategies, catering to different storage needs and optimization requirements.
//!
//! The library offers several caching strategies out of the box:
//!
//! * **Memory**: This strategy stores all the data directly in memory. It is ideal for smaller sets of data that need to be accessed frequently and quickly.
//! * **Disk**: This strategy saves data exclusively to disk storage. It is best suited for large data sets that don't need to be accessed as often or as swiftly.
//! * **Hybrid**: This strategy is a combination of memory and disk storage. It stores data in memory first, and swaps to disk for files that don't fit the memory limit.
//!
//! We also offer opt-in support for data compression:
//!
//! * **zstd**: Enabled using the `comp_zstd` feature flag.
//! * more to come...
//!
//! This crate is intended to be versatile, serving as an efficient solution whether you're developing a high-load system that needs to reduce database pressure, an application that requires quick access to binary data, or any other situation where efficient caching strategies are vital.
//!
//! ## Usage
//!
//! Add `bincache` to your `Cargo.toml` dependencies:
//!
//! ```sh,no_run
//! cargo add bincache                            # for stdlib I/O
//! cargo add bincache --features rt_tokio_1      # for tokio I/O
//! cargo add bincache --features rt_async-std_1  # for async-std I/O
//! ```
//!
//! ## Examples
//!
//! Getting started quickly using ready-made aliased cache builders:
//!
//! ```
//! use bincache::MemoryCacheBuilder;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut cache = MemoryCacheBuilder::default().build()?;
//! cache.put("key", b"value".to_vec()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! More advanced usage, using the builder directly:
//!
//! ```
//! use bincache::{Cache, CacheBuilder, MemoryStrategy};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut cache = CacheBuilder::default()
//!     .with_strategy(MemoryStrategy::default())
//!     .build()?;
//! cache.put("key", b"value".to_vec()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## License
//!
//! bincache is licensed under the MIT license.
//!

#[cfg(not(any(
    feature = "implicit-blocking",
    feature = "blocking",
    feature = "rt_tokio_1",
    feature = "rt_async-std_1"
)))]
compile_error!(
    "Cannot run without an async runtime.\nPlease enable one of the following features: [blocking, rt_tokio_1, async-std1]."
);

#[cfg(any(
    all(feature = "blocking", feature = "rt_tokio_1"),
    all(feature = "blocking", feature = "rt_async-std_1"),
    all(feature = "rt_tokio_1", feature = "rt_async-std_1")
))]
compile_error!("Cannot enable multiple async runtime features at the same time.");

mod builder;
mod cache;
pub mod compression;
pub mod error;
mod macros;
pub mod strategies;
pub mod traits;
pub mod utils;

pub(crate) use error::Result;
pub(crate) use utils::disk_util as DiskUtil;

macros::reexport_strategy!(Disk);
macros::reexport_strategy!(Hybrid);
macros::reexport_strategy!(Memory);

// Export basic types
pub use builder::CacheBuilder;
pub use cache::Cache;
pub use error::Error;

// README doctests
#[doc = include_str!("../../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;
