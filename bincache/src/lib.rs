//! # bincache
//!
//! `bincache` is a versatile, high-performance, async-first binary data caching library for Rust, designed with a focus on flexibility, efficiency, and ease of use. It enables developers to store, retrieve, and manage binary data using various caching strategies, catering to different storage needs and optimization requirements.
//!
//! The library offers several caching strategies out of the box:
//!
//! * **Memory**: This strategy stores all the data directly in memory. It is ideal for smaller sets of data that need to be accessed frequently and quickly.
//! * **Disk**: This strategy saves data exclusively to disk storage. It is best suited for large data sets that don't need to be accessed as often or as swiftly.
//! * **Hybrid**: This strategy is a combination of memory and disk storage. Frequently accessed data is stored in memory for fast access, while less frequently accessed data is moved to disk storage. This strategy provides a balanced approach for many use cases.
//!
//! Beyond these strategies, `bincache` provides various configuration options allowing for fine-tuned control over data storage and retrieval. These options include adjustable cache size, eviction policies, data expiry, and more.
//!
//! This crate is intended to be versatile, serving as an efficient solution whether you're developing a high-load system that needs to reduce database pressure, an application that requires quick access to binary data, or any other situation where efficient caching strategies are vital.
//!
//! ## Usage
//!
//! Add `bincache` to your `Cargo.toml` dependencies:
//!
//! ```bash,no_run
//! cargo add bincache
//! ```
//!
//! Then simply create a cache using the relevant `CacheBuilder`:
//!
//! ```
//! use bincache::MemoryCacheBuilder;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() {
//!     let mut cache = MemoryCacheBuilder::new().build().unwrap();
//!     cache.put("key", b"value".to_vec()).await.unwrap();
//! }
//! ```
//!
//! Or use the generic `CacheBuilder` to create a cache with a custom strategy:
//!
//! ```
//! use bincache::{Cache, CacheBuilder, MemoryStrategy};
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() {
//!     let mut cache = CacheBuilder::default()
//!         .with_strategy(MemoryStrategy::default())
//!         .build()
//!         .unwrap();
//!     cache.put("key", b"value".to_vec()).await.unwrap();
//! }
//! ```
//!
//! ## License
//!
//! bincache is licensed under the MIT license.
//!

#[cfg(all(feature = "blocking", feature = "tokio_rt_1"))]
compile_error!("Cannot enable both the `blocking` and `tokio_rt_1` features at the same time.");

#[cfg(test)]
mod test_utils;

mod builder;
mod cache;
mod error;
mod macros;
mod strategies;
mod traits;

pub(crate) use error::Result;

reexport_strategy!(Disk);
reexport_strategy!(Hybrid);
reexport_strategy!(Memory);

// Export basic types
pub use builder::CacheBuilder;
pub use cache::Cache;
pub use error::Error;

// README doctests
#[doc = include_str!("../../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;
