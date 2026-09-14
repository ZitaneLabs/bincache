# bincache

![Backed by Zitane Labs][badge_zitane]
![Powered by Rust][badge_rust]
![crates.io: bincache][badge_crates]
![MSRV: 1.85][badge_msrv]
![License: MIT][badge_license]

[badge_zitane]: https://badgers.space/badge/Backed%20by/Zitane%20Labs/pink
[badge_rust]: https://badgers.space/badge/Powered%20by/Rust/orange
[badge_crates]: https://badgers.space/crates/info/bincache
[badge_msrv]: https://badgers.space/badge/MSRV/1.85
[badge_license]: https://badgers.space/badge/License/MIT

An async API for caching binary values in memory, on disk, or across both tiers,
with optional compression and explicit capacity limits.

```sh
cargo add bincache
```

## Performance and strategy selection

| Strategy | Intended trade-off | Placement and lifetime |
| --- | --- | --- |
| Memory | Lowest expected access latency; limited by available RAM | Volatile. Uncompressed reads can borrow stored bytes. |
| Disk | Higher I/O latency; suitable for larger objects and recoverable caching | Writes files; reads allocate and load the complete entry. Files survive dropping the cache. |
| Hybrid | Balances RAM use with disk storage | Each new value goes to memory if it fits, otherwise disk. Existing entries are not evicted to make room. |

Hybrid reads leave entries in their current tier. Replacing a key chooses memory
first again, so a replacement can move in either direction. `flush()` moves all
memory entries to disk when disk limits permit. Only disk entries are recoverable;
dropping a hybrid cache does **not** flush it. The default hybrid memory tier is
unlimited, so configure limits to get spillover.

Compression can reduce stored memory/disk bytes at the cost of CPU time,
allocations, and write/read latency. Incompressible inputs may grow. Gzip, Brotli,
and Zstandard have different speed/size trade-offs, which also depend on the
compression level and input. Compression runs within the operation's future; an
async API does not offload this CPU work automatically.

Use the [benchmark suite](https://github.com/ZitaneLabs/bincache/tree/main/bincache/benches)
to measure your workload and hardware. It covers 1 KiB, 64 KiB, and 1 MiB values,
all storage strategies and I/O modes, codec latency, and stored sizes. These are
intended characteristics, not measured speed rankings or latency guarantees.

## Memory capacity limits

Limits reject writes with `Error::LimitExceeded`; there is no automatic eviction,
TTL, or LRU policy. Here both the byte and entry limits apply:

```rust
use bincache::{CacheBuilder, MemoryStrategy};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut cache = CacheBuilder
    .with_strategy(MemoryStrategy::new(Some(8 * 1024 * 1024), Some(1_000)))
    .build().await?;

cache.put("thumbnail:42", vec![7; 1024]).await?;
assert_eq!(cache.get("thumbnail:42").await?.len(), 1024);
cache.put("thumbnail:42", vec![8; 512]).await?; // replaces the same key
assert_eq!(cache.capacity().unwrap().used(), 512);
let thumbnail = cache.take("thumbnail:42").await?; // returns and removes
assert_eq!(thumbnail, vec![8; 512]);
assert!(!cache.exists("thumbnail:42"));
# Ok(())
# }
```

`MemoryCacheBuilder::default()` is a shortcut for unlimited, uncompressed memory
storage. `CacheBuilder` accepts a strategy and optional compressor in either order.

## Disk caching and recovery after restart

Use a dedicated directory on persistent storage in an application. This runnable
example uses `tempfile` (a dev dependency) to isolate its files. A new instance
starts with an empty index; call `recover()` once before using existing files.

```rust
use bincache::{Cache, DiskStrategy, NO_COMPRESSION};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let directory = tempfile::tempdir()?;
let strategy = || DiskStrategy::new(directory.path(), Some(256 * 1024 * 1024), None);
let mut cache = Cache::new(strategy(), NO_COMPRESSION).await?;
cache.put(42_u64, b"downloaded response".to_vec()).await?;
drop(cache); // leaves files on disk

let mut restarted = Cache::new(strategy(), NO_COMPRESSION).await?;
let recovered = restarted.recover(|key| key.parse::<u64>().ok()).await?;
assert_eq!(recovered, 1);
assert_eq!(restarted.get(42).await?.as_ref(), b"downloaded response");
restarted.delete(42).await?; // removes the file as well as the index entry
# Ok(())
# }
```

For an application directory, construct the strategy with, for example,
`DiskStrategy::new(std::path::Path::new("/var/cache/my-app"), None, None)`.
Building creates missing directories and can fail if permissions deny access.
Use the same key conversion and compression algorithm when reopening a cache.

## Hybrid caching

```rust
use bincache::{Cache, HybridStrategy, NO_COMPRESSION, strategies::Limits};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let directory = tempfile::tempdir()?;
let mut cache = Cache::new(
    HybridStrategy::new(
        directory.path(),
        Limits::new(Some(64 * 1024), Some(100)), // memory tier
        Limits::new(Some(256 * 1024 * 1024), None), // disk tier
    ),
    NO_COMPRESSION,
).await?;

cache.put("small", vec![1; 1024]).await?; // memory
cache.put("large", vec![2; 1024 * 1024]).await?; // disk
assert_eq!(cache.flush().await?, 1); // moves "small" to disk
// Now both entries can be recovered by a new instance using this directory.
# Ok(())
# }
```

`flush()` is a tier migration, not a transaction or a general durability barrier.
Reserve enough disk capacity for the memory entries before flushing.

## Compression

```sh
cargo add bincache --features comp_gzip
```

```rust
# #[cfg(feature = "comp_gzip")]
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use bincache::{MemoryCacheBuilder, compression::{CompressionLevel, Gzip}};

let mut cache = MemoryCacheBuilder::default()
    .with_compression(Gzip::new(CompressionLevel::Fastest))
    .build().await?;
cache.put("response", vec![b'a'; 64 * 1024]).await?;
assert_eq!(cache.get("response").await?.as_ref(), vec![b'a'; 64 * 1024]);
# Ok(())
# }
# #[cfg(not(feature = "comp_gzip"))]
# fn main() {}
```

Substitute `Brotli` (`comp_brotli`) or `Zstd` (`comp_zstd`) in the same builder.
Compression applies before placement and capacity checks; every `get`/`take`
decompresses the stored bytes. There is no retained decompressed copy.

## Tokio

```toml
[dependencies]
bincache = { version = "0.5", default-features = false, features = ["rt_tokio_1"] }
tokio = { version = "1", features = ["macros", "rt"] }
```

```rust
# #[cfg(feature = "rt_tokio_1")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut cache = bincache::Cache::new(
        bincache::DiskStrategy::new(directory.path(), None, None),
        bincache::NO_COMPRESSION,
    ).await?;
    cache.put("response", b"from a Tokio task".to_vec()).await?;
    assert_eq!(cache.get("response").await?.as_ref(), b"from a Tokio task");
    Ok(())
}
# #[cfg(not(feature = "rt_tokio_1"))]
# fn main() {}
```

## async-std

```toml
[dependencies]
bincache = { version = "0.5", default-features = false, features = ["rt_async-std_1"] }
async-std = { version = "1", features = ["attributes"] }
```

```rust
# #[cfg(feature = "rt_async-std_1")]
#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut cache = bincache::Cache::new(
        bincache::DiskStrategy::new(directory.path(), None, None),
        bincache::NO_COMPRESSION,
    ).await?;
    cache.put("response", b"from an async-std task".to_vec()).await?;
    assert_eq!(cache.get("response").await?.as_ref(), b"from an async-std task");
    Ok(())
}
# #[cfg(not(feature = "rt_async-std_1"))]
# fn main() {}
```

The disk examples above use `tempfile = "3"` as an additional dependency for their
temporary directories; replace it with your persistent application path.

## Semantics and guarantees

- **Replacement:** `put` replaces an equal key. Built-in strategies check the new
  byte total after subtracting the old value, without consuming another entry
  slot. Rejected replacements retain the previous value. Custom strategies must
  implement `replace`; its default returns an error.
- **Capacity:** `None` means no configured limit for that dimension; `Some(0)`
  permits no entries or no payload bytes, respectively. Byte limits count stored
  payload lengths after compression, excluding keys, allocations, indexes, file
  headers, temporary files, and filesystem overhead. They are not RAM or disk
  quotas. Replacement and compression can temporarily allocate more memory.
- **`capacity()`:** returns a snapshot of the configured byte limit and tracked
  stored payload bytes. Memory/disk return `None` without a byte limit, even with
  an entry limit. Hybrid reports the sum of both tiers only when **both** byte
  limits exist. Entry counts and individual tier usage are not exposed here.
  Zero total capacity yields `NaN` utilization when usage is also zero; recovery
  can make usage exceed the configured total.
- **Recovery:** best-effort, intended once on a fresh cache. It rebuilds the index
  from files, without enforcing limits or validating/decompressing payloads.
  Repeated recovery or recovery into a populated cache can double-count usage or
  replace index entries. Directory listing entry errors are skipped; a file read
  error can abort after accounting has already changed. Recreate the instance
  after a failed recovery rather than retrying on the same instance.
- **Partial/corrupted files:** recognizable but malformed headers and keys your
  callback rejects are moved to `lost+found` when possible. Other files are tried
  as legacy entries using their filename as the key; even temporary files may be
  considered. Legacy files can be indexed but are not guaranteed readable by the
  current `get` implementation. There is no payload length/checksum validation:
  truncation or corruption can cause a later read/decompression error, or return
  damaged bytes undetected. Recovery is not an integrity check.
- **Persistence and atomicity:** disk writes use a sibling temporary file, sync
  its data, then rename it over the destination. Rename behavior depends on the
  platform/filesystem; the parent directory is not synced. Dropping the cache
  preserves disk files, but crash/power-loss durability is not guaranteed.
  Operations are not transactions and are not guaranteed cancellation-safe.
  `take` and `delete` remove the index entry before I/O/decompression completes;
  failure can leave a missing key, an orphaned file, or stale accounting.
  `flush` can leave files and changed accounting after partial failure.
- **Concurrency:** mutation requires `&mut Cache`; the cache has no internal
  lock. Its `Send`/`Sync` properties depend on the key, strategy, entry, and
  compressor; built-in combinations with suitable keys can be shared across
  tasks using an `Arc` and an async mutex or read/write lock. Hold the lock for
  the complete operation, including `.await`. Convert a borrowed `get` result
  with `.into_owned()` before releasing a read guard. Shared reads are possible
  with `&Cache`, but writers need exclusive access. Use one live cache instance
  per disk directory: there is no coordination between instances or processes.
- **Disk format:** an implementation detail, not a stable interchange or archival
  format. There is no cross-version compatibility guarantee or persisted
  compressor configuration. Keep authoritative data elsewhere and be prepared
  to discard/rebuild the cache. Key strings must be unique and deterministic;
  different keys with the same string target the same disk file.

## Custom strategies

Implement [`CacheStrategy`](https://docs.rs/bincache/latest/bincache/trait.CacheStrategy.html)
for custom storage, and optionally `RecoverableStrategy` / `FlushableStrategy`.
The trait documentation includes a complete custom storage example, including
replacement and borrowing behavior.

Implement [`CompressionStrategy`](https://docs.rs/bincache/latest/bincache/trait.CompressionStrategy.html)
for a custom codec. Its documentation includes a runnable adapter example.
Both extension traits use `async-trait`; add `async-trait = "0.1"` to implement them.

## Feature flags

| Feature | Behavior |
| --- | --- |
| `implicit-blocking` (default) | Uses blocking standard-library file I/O unless a runtime feature is selected. The API still returns futures. |
| `blocking` | Explicit blocking I/O; cannot be combined with a runtime feature. |
| `rt_tokio_1` | Tokio file I/O; requires a Tokio runtime for disk operations. |
| `rt_async-std_1` | async-std file I/O. |
| `comp_gzip` | Exports the Gzip compressor. |
| `comp_brotli` | Exports the Brotli compressor. |
| `comp_zstd` | Exports the Zstandard compressor. |

Choose one I/O mode. Tokio and async-std cannot be enabled together, so
`--all-features` is invalid. Prefer `--no-default-features` when selecting a
runtime explicitly. Blocking mode can be driven by any executor, but blocks its
thread during I/O. Some disk bookkeeping, including rename and directory
traversal during recovery, uses synchronous filesystem calls in every mode.

## Development

The [benchmark guide](https://github.com/ZitaneLabs/bincache/blob/main/bincache/benches/README.md)
includes reproducible commands, methodology, size reports, and baseline comparisons.
CI checks tests/doctests, benchmark builds, clippy, formatting, MSRV, and dependency
advisories/licenses separately for compatible feature sets.

Licensed under the MIT license.
