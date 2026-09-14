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

## Real-world usage

The [usage guide](bincache/README.md), also available as the
[crate documentation](https://docs.rs/bincache/latest/bincache/), contains tested examples for:

- [Disk caching and recovery after restart](bincache/README.md#disk-caching-and-recovery-after-restart).
- [Hybrid caching and explicit flushing](bincache/README.md#hybrid-caching).
- [Compression configuration](bincache/README.md#compression).
- [Tokio](bincache/README.md#tokio) and [async-std](bincache/README.md#async-std).
- Custom [`CacheStrategy`](https://docs.rs/bincache/latest/bincache/trait.CacheStrategy.html)
  and [`CompressionStrategy`](https://docs.rs/bincache/latest/bincache/trait.CompressionStrategy.html)
  implementations in their trait documentation.

## Semantics and guarantees

`put` replaces an existing value. Limits reject writes instead of evicting entries;
byte limits count payload bytes **after compression**, excluding keys, allocations,
indexes and filesystem overhead. `capacity()` reports the configured byte limit
and tracked payload usage. Memory/disk report `None` without a byte limit; hybrid
reports a sum only when both tiers have byte limits. Entry-only limits are not
represented.

Disk files survive dropping the cache, but constructing a new cache does not
recover them automatically. Call `recover()` once on a fresh instance with the
same key mapping and codec. Recovery is best-effort: it does not enforce capacity
limits or verify payload integrity. Malformed files may be quarantined, rejected,
or indexed without being readable. Cache data is an implementation detail without
a cross-version format guarantee.

Use one live instance per disk directory. Mutations require `&mut Cache`; sharing
between tasks requires your own synchronization. There are no transactions or
cancellation-safety guarantees. Disk writes sync file data before rename, but do
not sync the parent directory or guarantee crash/power-loss durability. Failed
`take`, `delete`, `recover`, or `flush` operations can leave partial state.
See the [full semantics and failure behavior](bincache/README.md#semantics-and-guarantees)
before relying on persistence or recovery.

## Feature flags

| Feature | Behavior |
| --- | --- |
| `implicit-blocking` (default) | Blocking standard-library file I/O unless a runtime feature is selected; the API still returns futures. |
| `blocking` | Explicit blocking I/O; cannot be combined with runtime features. |
| `rt_tokio_1` | Tokio file I/O. |
| `rt_async-std_1` | async-std file I/O. |
| `comp_gzip` | Gzip compression. |
| `comp_brotli` | Brotli compression. |
| `comp_zstd` | Zstandard compression. |

Choose one I/O mode. Tokio and async-std cannot be enabled together, so
`--all-features` is invalid. Prefer `--no-default-features` with an explicit runtime.
Compression runs on the calling task; some disk bookkeeping uses synchronous
filesystem calls in every mode.

## Development

The [benchmark guide](bincache/benches/README.md) covers reproducible commands,
measurement boundaries, stored-size reports and baseline comparisons. CI checks
tests/doctests, benchmark builds and smoke tests, clippy, formatting, MSRV, and
dependency advisories/licenses across compatible feature sets.

Licensed under the MIT license.
