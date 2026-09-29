use crate::{Cache, CacheKey, CacheStrategy, Error, NO_COMPRESSION, Noop, RecoverableStrategy};

pub(super) async fn new_cache<K, S>(strategy: S) -> Cache<K, S, Noop>
where
    K: CacheKey + Eq + std::hash::Hash + Sync + Send,
    S: CacheStrategy + Send,
{
    Cache::new(strategy, NO_COMPRESSION).await.unwrap()
}

pub(super) async fn basic<S: CacheStrategy + Send>(
    strategy: S,
    counts: impl Fn(&S) -> (usize, usize),
) {
    let mut cache = new_cache(strategy).await;
    for (key, entries) in [("foo", 1), ("bar", 2)] {
        cache.put(key, key.as_bytes()).await.unwrap();
        assert_eq!(counts(cache.strategy()), (entries * 3, entries));
    }
    for key in ["foo", "bar"] {
        assert_eq!(cache.get(key).await.unwrap(), key.as_bytes());
    }
    assert!(cache.get("baz").await.is_err());
    for (key, entries) in [("foo", 1), ("bar", 0)] {
        cache.delete(key).await.unwrap();
        assert_eq!(counts(cache.strategy()), (entries * 3, entries));
    }
}

pub(super) async fn limit<S: CacheStrategy + Send>(strategy: S, expected: &str) {
    let mut cache = new_cache(strategy).await;
    for key in ["foo", "bar"] {
        cache.put(key, key.as_bytes()).await.unwrap();
    }
    for key in ["foo", "bar"] {
        assert_eq!(cache.get(key).await.unwrap(), key.as_bytes());
    }
    assert!(matches!(
        cache.put("baz", b"baz".as_slice()).await,
        Err(Error::LimitExceeded { limit_kind }) if limit_kind == expected
    ));
}

pub(super) async fn replacement<S: CacheStrategy + Send>(
    strategy: S,
    counts: impl Fn(&S) -> (usize, usize),
) {
    let mut cache = new_cache(strategy).await;
    for (value, size) in [(1, 100), (2, 50), (5, 75), (2, 50)] {
        cache.put("foo", vec![value; size]).await.unwrap();
        assert_eq!(cache.get("foo").await.unwrap(), vec![value; size]);
        assert_eq!(counts(cache.strategy()), (size, 1));
    }
    cache.put("bar", vec![3; 50]).await.unwrap();
    assert!(cache.put("foo", vec![4; 51]).await.is_err());
    assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
    assert_eq!(counts(cache.strategy()), (100, 2));
}

pub(super) async fn recovery<S: RecoverableStrategy + Send>(
    strategy: S,
    restarted: S,
    counts: impl Fn(&S) -> (usize, usize),
    initial: &[&str],
    recovered: &[&str],
) {
    let mut cache = new_cache(strategy).await;
    for key in initial {
        cache.put(key.to_string(), key.as_bytes()).await.unwrap();
    }
    drop(cache);
    let mut cache = new_cache(restarted).await;
    assert_eq!(
        cache.recover(|key| Some(key.to_owned())).await.unwrap(),
        recovered.len()
    );
    assert_eq!(
        counts(cache.strategy()),
        (recovered.len() * 3, recovered.len())
    );
    for key in initial {
        assert_eq!(cache.exists(key.to_string()), recovered.contains(key));
    }
}
