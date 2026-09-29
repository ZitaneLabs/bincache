use std::{fs, io::ErrorKind, path::Path};

use crate::{
    Cache, NO_COMPRESSION, RecoverableStrategy,
    utils::test::{IO_FAILURES, TempDir},
};

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
    let replacement = b"new";
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

    for (key, extension, data) in [
        // Simulate a crash after sync but before rename, for insert and replace.
        ("foo", "tmp", encode_entry("foo", b"uncommitted")),
        ("new", "tmp", encode_entry("new", b"uncommitted")),
        ("partial", "tmp", b"BIN".to_vec()),
        // Malformed committed records and key/filename mismatches are best-effort.
        ("corrupt", "", b"BINCACHE".to_vec()),
        ("mismatch", "", encode_entry("foo", b"wrong")),
        ("rejected", "", encode_entry("rejected", b"value")),
    ] {
        fs::write(
            cache_path(dir.as_ref(), key).with_extension(extension),
            data,
        )
        .unwrap();
    }

    // An unreadable valid record stays in place; later restarts recover it.
    let path = cache_path(dir.as_ref(), "foo");
    IO_FAILURES
        .lock()
        .unwrap()
        .push((path.clone(), ErrorKind::PermissionDenied));
    for unreadable in [true, false, false] {
        let mut cache = Cache::new(strategy(dir.as_ref()), NO_COMPRESSION)
            .await
            .unwrap();
        assert_eq!(
            cache
                .recover(|key| (key != "rejected").then(|| key.to_owned()))
                .await
                .unwrap(),
            usize::from(!unreadable)
        );
        assert!(path.is_file());
        if unreadable {
            assert_eq!(counts(cache.strategy()), (0, 0));
            IO_FAILURES.lock().unwrap().retain(|(p, _)| p != &path);
            continue;
        }
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
