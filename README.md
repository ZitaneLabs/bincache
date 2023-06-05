![Backed by Zitane Labs][badge_zitane]
![Powered by Rust][badge_rust]
![License: MIT][badge_license]

# bincache
> A versatile binary caching library.

[badge_zitane]: https://badgers.space/badge/Backed%20by/Zitane%20Labs/pink
[badge_rust]: https://badgers.space/badge/Powered%20by/Rust/orange
[badge_license]: https://badgers.space/badge/License/MIT

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
    ```plain,no_run
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
