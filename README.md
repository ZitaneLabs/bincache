# bincache

![Backed by Zitane Labs][badge_zitane]
![Powered by Rust][badge_rust]
![License: MIT][badge_license]

[badge_zitane]: https://badgers.space/badge/Backed%20by/Zitane%20Labs/pink
[badge_rust]: https://badgers.space/badge/Powered%20by/Rust/orange
[badge_license]: https://badgers.space/badge/License/MIT

**The library is not yet published to crates.io.**<br>
The API is not yet stabilized, so expect breaking changes.

## Features

- Simple API
- Flexible cache sizing, limiting, and eviction
- Multiple cache strategies for different use cases
- Support for cache compression
- Best-effort cache recovery

### Cache Strategies
Bincache uses a strategy pattern to allow for different caching strategies:

- [x] In-memory cache
- [x] Disk-backed cache
- [x] Hybrid cache (in-memory + disk-backed)
- [x] Custom strategies possible through `CacheStrategy`

## Usage

1. Add `bincache` to your project:
    ```bash,no_run
    cargo add bincache                            # use blocking I/O
    cargo add bincache --features rt_tokio_1      # enable tokio 1.x support
    cargo add bincache --features rt_async-std_1  # enable async-std 1.x support
    ```

2. Create a `Cache` instance with your preferred strategy:
    ```rust
    #[tokio::main(flavor = "current_thread")]
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        let mut cache = bincache::MemoryCacheBuilder::default().build()?;

        // Put a key-value pair into the cache
        cache.put(&"foo", b"foo".to_vec()).await?;

        // Read the value back out
        let foo = cache.get(&"foo").await?;

        // Make sure it's the same
        assert_eq!(foo, b"foo".as_slice());

        Ok(())
    }
    ```
3. That's it!

## Features

- `blocking` - Enables blocking stdlib I/O
- `rt_tokio_1` - Enables tokio 1.x support
- `rt_async-std_1` - Enables async-std 1.x support
- `comp_zstd` - Enables zstd compression support

> By default, we enable a "soft" `implicit-blocking` feature, which only uses blocking I/O if no other runtime feature is enabled.
>
> You can explicitly opt-in to blocking I/O by enabling the `blocking` feature, which will disallow the use of `rt_tokio_1` and `rt_async-std_1`.
