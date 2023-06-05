# bincache
> A versatile binary caching library.

![](https://badgers.space/badge/Powered%20by/Rust/black?labelColor=orange&icon=https://www.rust-lang.org/static/images/rust-logo-blk.svg)
![](https://badgers.space/badge/License/MIT)

## Features

- Simple API
- Flexible cache sizing, limiting, and eviction
- Multiple cache strategies for different use cases
- Best-effort cache recovery

### Cache Strategies
Bincache uses a strategy pattern to allow for different caching strategies:

- [x] In-memory cache
- [x] Disk-backed cache
- [x] Hybrid cache (in-memory + disk-backed)
- [x] Custom strategies possible through `CacheStrategy`

## Usage

1. Add `bincache` to your project:
    ```
    cargo add bincache
    ```

2. Create a `Cache` instance with your preferred strategy:
    ```rust
    fn main() -> Result<(), Box<dyn std::error::Error>> {
        let mut cache = bincache::MemoryCacheBuilder::new().build()?;

        // Put a key-value pair into the cache
        cache.put(&"foo", b"foo".to_vec())?;

        // Read the value back out
        let foo = cache.get(&"foo")?;

        // Make sure it's the same
        assert_eq!(foo, b"foo".as_slice());

        Ok(())
    }
    ```
3. That's it!
