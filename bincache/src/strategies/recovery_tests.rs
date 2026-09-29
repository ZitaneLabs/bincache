use std::{fs, io::ErrorKind, path::Path};

use crate::{
    RecoverableStrategy,
    utils::test::{IO_FAILURES, TempDir},
};

use super::{
    disk::{FORMAT_DIR, cache_path, encode_entry},
    test_helpers::new_cache,
};

#[cfg(feature = "rt_tokio_1")]
pub(super) async fn canceled_write<S: RecoverableStrategy + Send>(
    strategy: impl Fn(&Path) -> S,
    pause: impl Fn(&S) -> std::sync::Arc<super::writes::CommitPause>,
) {
    for action in ["insert", "read", "take", "delete", "retry"] {
        let dir = TempDir::new();
        let mut cache = new_cache(strategy(dir.as_ref())).await;
        if action != "insert" {
            cache.put("foo", b"old".to_vec()).await.unwrap();
        }
        let used = cache.capacity().unwrap().used();
        let gate = pause(cache.strategy());
        let mut write = Box::pin(cache.put("foo", vec![9; 9]));
        tokio::select! {
            result = &mut write => panic!("write completed before cancellation: {result:?}"),
            _ = gate.reached.notified() => {},
        }
        drop(write);
        assert_eq!(
            fs::read(cache_path(dir.as_ref(), "foo")).unwrap(),
            encode_entry("foo", &[9; 9])
        );
        assert_eq!(cache.capacity().unwrap().used(), used);
        // A canceled write on foo must not stall another path.
        cache.put("other", b"other".to_vec()).await.unwrap();
        gate.resume.notify_one();
        match action {
            "read" => {
                let (first, second) = tokio::join!(cache.get("foo"), cache.get("foo"));
                assert_eq!(first.unwrap(), b"old".as_slice());
                assert_eq!(second.unwrap(), b"old".as_slice());
                assert_eq!(cache.take("foo").await.unwrap(), b"old");
            }
            "take" => assert_eq!(cache.take("foo").await.unwrap(), b"old"),
            "delete" => cache.delete("foo").await.unwrap(),
            _ => {
                cache.put("foo", b"retry".to_vec()).await.unwrap();
                assert_eq!(cache.capacity().unwrap().used(), 10);
                assert_eq!(cache.take("foo").await.unwrap(), b"retry");
            }
        }
        assert!(!cache_path(dir.as_ref(), "foo").exists());
        assert_eq!(cache.capacity().unwrap().used(), 5);
        cache.delete("other").await.unwrap();
        assert_eq!(cache.capacity().unwrap().used(), 0);
        drop(cache);
        let mut recovered = new_cache(strategy(dir.as_ref())).await;
        assert_eq!(
            recovered.recover(|key| Some(key.to_owned())).await.unwrap(),
            0
        );
    }
}

pub(super) async fn replaces_normalized_key<S: RecoverableStrategy + Send>(
    strategy: impl Fn(&Path) -> S,
    counts: impl Fn(&S) -> (usize, usize),
) {
    for take in [false, true] {
        let dir = TempDir::new();
        let mut cache = new_cache(strategy(dir.as_ref())).await;
        cache.put("FOO".to_owned(), b"old".to_vec()).await.unwrap();
        drop(cache);

        let mut cache = new_cache(strategy(dir.as_ref())).await;
        assert_eq!(
            cache.recover(|key| Some(key.to_lowercase())).await.unwrap(),
            1
        );
        cache
            .put("foo".to_owned(), b"replacement".to_vec())
            .await
            .unwrap();
        assert_eq!(
            cache.get("foo".to_owned()).await.unwrap(),
            b"replacement".as_slice()
        );
        assert_eq!(counts(cache.strategy()), (11, 1));
        drop(cache);

        let mut cache = new_cache(strategy(dir.as_ref())).await;
        assert_eq!(
            cache.recover(|key| Some(key.to_lowercase())).await.unwrap(),
            1
        );
        assert_eq!(counts(cache.strategy()), (11, 1));
        let path = cache_path(dir.as_ref(), "FOO");
        IO_FAILURES
            .lock()
            .unwrap()
            .push((path.clone(), ErrorKind::StorageFull));
        // Deletion must succeed even when copying the existing payload cannot.
        if take {
            assert_eq!(cache.take("foo".to_owned()).await.unwrap(), b"replacement");
        } else {
            cache.delete("foo".to_owned()).await.unwrap();
        }
        IO_FAILURES.lock().unwrap().retain(|(p, _)| p != &path);
        assert_eq!(counts(cache.strategy()), (0, 0));
        assert!(!cache_path(dir.as_ref(), "FOO").exists());
        assert!(!cache_path(dir.as_ref(), "foo").exists());
    }
}

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
    let mut cache = new_cache(strategy(dir.as_ref())).await;
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

    let mut cache = new_cache(strategy(dir.as_ref())).await;
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
    let mut cache = new_cache(strategy(dir.as_ref())).await;
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
        let mut cache = new_cache(strategy(dir.as_ref())).await;
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
