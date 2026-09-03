use std::{fs, path::Path};

use crate::{Cache, NO_COMPRESSION, RecoverableStrategy, utils::test::TempDir};

use super::disk::{FORMAT_DIR, cache_path, encode_entry};

pub(super) async fn ignores_old_formats<S: RecoverableStrategy + Send>(
    strategy: impl Fn(&Path) -> S,
    counts: impl Fn(&S) -> (usize, usize),
) {
    let dir = TempDir::new();
    let encoded = encode_entry("embedded-key", b"embedded-value");
    let hash = blake3::hash(b"embedded-key").to_hex().to_string();
    let old_files = [
        ("foo", b"old".to_vec()),
        ("magic", b"BINCACHE".to_vec()),
        ("record", encoded.clone()),
        (hash.as_str(), encoded.clone()),
        ("legacy.tmp", encoded),
    ];
    for (key, value) in &old_files {
        fs::write(dir.as_ref().join(key), value).unwrap();
    }
    let mut cache = Cache::new(strategy(dir.as_ref()), NO_COMPRESSION)
        .await
        .unwrap();
    assert_eq!(
        cache
            .recover(|_| panic!("old files must not reach key recovery"))
            .await
            .unwrap(),
        0
    );
    assert_eq!(counts(cache.strategy()), (0, 0));

    // Repopulating and replacing an old key creates only a v1 entry.
    cache.put("foo".to_owned(), b"new".to_vec()).await.unwrap();
    let replacement = b"BINCACHE replacement";
    cache
        .put("foo".to_owned(), replacement.to_vec())
        .await
        .unwrap();
    assert_eq!(counts(cache.strategy()), (replacement.len(), 1));
    drop(cache);

    let mut cache = Cache::new(strategy(dir.as_ref()), NO_COMPRESSION)
        .await
        .unwrap();
    assert_eq!(cache.recover(|key| Some(key.to_owned())).await.unwrap(), 1);
    assert_eq!(counts(cache.strategy()), (replacement.len(), 1));
    assert_eq!(cache.take("foo".to_owned()).await.unwrap(), replacement);
    assert_eq!(counts(cache.strategy()), (0, 0));
    for (key, value) in &old_files {
        assert_eq!(fs::read(dir.as_ref().join(key)).unwrap(), *value);
    }
}

pub(super) async fn interrupted<S: RecoverableStrategy + Send>(
    strategy: impl Fn(&Path) -> S,
    counts: impl Fn(&S) -> (usize, usize),
) {
    let dir = TempDir::new();
    let mut cache = Cache::new(strategy(dir.as_ref()), NO_COMPRESSION)
        .await
        .unwrap();
    // A failed rename must clean up the staged file without counting the entry.
    let blocked = cache_path(dir.as_ref(), "blocked");
    fs::create_dir(&blocked).unwrap();
    assert!(
        cache
            .put("blocked".to_owned(), b"value".to_vec())
            .await
            .is_err()
    );
    assert!(!cache.exists("blocked".to_owned()));
    assert_eq!(counts(cache.strategy()), (0, 0));
    assert!(!blocked.with_extension("tmp").exists());
    fs::remove_dir(blocked).unwrap();

    cache
        .put("foo".to_owned(), b"committed".to_vec())
        .await
        .unwrap();
    drop(cache);

    // Simulate a crash after sync but before rename, for both insert and replace.
    for key in ["foo", "new"] {
        fs::write(
            cache_path(dir.as_ref(), key).with_extension("tmp"),
            encode_entry(key, b"uncommitted"),
        )
        .unwrap();
    }
    fs::write(
        cache_path(dir.as_ref(), "partial").with_extension("tmp"),
        b"BIN",
    )
    .unwrap();
    // Malformed committed records and key/filename mismatches remain best-effort.
    fs::write(cache_path(dir.as_ref(), "corrupt"), b"BINCACHE").unwrap();
    fs::write(
        cache_path(dir.as_ref(), "mismatch"),
        encode_entry("foo", b"wrong"),
    )
    .unwrap();
    fs::write(
        cache_path(dir.as_ref(), "rejected"),
        encode_entry("rejected", b"value"),
    )
    .unwrap();

    // Two restarts ensure quarantine cannot be scanned as live cache data.
    for _ in 0..2 {
        let mut cache = Cache::new(strategy(dir.as_ref()), NO_COMPRESSION)
            .await
            .unwrap();
        assert_eq!(
            cache
                .recover(|key| (key != "rejected").then(|| key.to_owned()))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            cache.get("foo".to_owned()).await.unwrap(),
            b"committed".as_slice()
        );
        assert!(!cache.exists("new".to_owned()));
        assert_eq!(counts(cache.strategy()), (9, 1));
    }
    let lost = dir.as_ref().join(FORMAT_DIR).join("lost+found");
    assert_eq!(fs::read_dir(lost).unwrap().count(), 6);
}
