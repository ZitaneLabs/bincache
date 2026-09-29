use std::{
    fs,
    future::Future,
    path::Path,
    task::{Context, Waker},
};

use super::{Hybrid, Limits, cache_path};
use crate::{
    strategies::{test_helpers::new_cache, writes::CommitPause},
    utils::test::TempDir,
};

fn strategy(path: &Path, memory: usize) -> Hybrid {
    Hybrid::new(
        path,
        Limits::new(Some(memory), None),
        Limits::new(Some(100), None),
    )
}

fn counts(strategy: &Hybrid) -> (usize, usize, usize, usize) {
    (
        strategy.memory_limits.current_byte_count,
        strategy.memory_limits.current_entry_count,
        strategy.disk_limits.current_byte_count,
        strategy.disk_limits.current_entry_count,
    )
}

async fn cancel<T>(operation: impl Future<Output = T>, pause: &CommitPause) {
    tokio::pin!(operation);
    tokio::select! {
        _ = &mut operation => panic!("operation completed before cancellation"),
        _ = pause.reached.notified() => {},
    }
}

fn cancel_pending<T>(operation: impl Future<Output = T>) {
    assert!(
        Box::pin(operation)
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
}

#[tokio::test]
async fn canceled_write_commit() {
    for memory in [0, 5] {
        crate::strategies::recovery_tests::canceled_write(
            |path| strategy(path, memory),
            |hybrid| hybrid.writes.pause_commit(0),
        )
        .await;
    }
}

#[tokio::test]
async fn canceled_flush_keeps_completed_entries() {
    let dir = TempDir::new();
    let mut cache = new_cache(strategy(dir.as_ref(), 20)).await;
    for key in ["first", "second"] {
        cache.put(key, key.as_bytes()).await.unwrap();
    }
    let pause = cache.strategy().writes.pause_commit(1);
    cancel(cache.flush(), &pause).await;
    let canceled = ["first", "second"]
        .into_iter()
        .find(|key| {
            cache
                .strategy()
                .writes
                .pending_path(cache_path(dir.as_ref(), key))
                .is_some()
        })
        .unwrap();
    let completed = if canceled == "first" {
        "second"
    } else {
        "first"
    };
    assert_eq!(
        counts(cache.strategy()),
        (canceled.len(), 1, completed.len(), 1)
    );
    pause.resume.notify_one();
    assert_eq!(cache.get(completed).await.unwrap(), completed.as_bytes());
    // Removal must settle the canceled write without an intervening read.
    cache.delete(canceled).await.unwrap();
    assert!(!cache_path(dir.as_ref(), canceled).exists());
    assert_eq!(counts(cache.strategy()), (0, 0, completed.len(), 1));
    drop(cache);
    let mut cache = new_cache(strategy(dir.as_ref(), 20)).await;
    assert_eq!(cache.recover(|key| Some(key.to_owned())).await.unwrap(), 1);
    assert_eq!(
        cache.get(completed.to_owned()).await.unwrap(),
        completed.as_bytes()
    );
}

#[tokio::test]
async fn canceled_promotion_and_removal_preserve_normalized_keys() {
    for take in [false, true] {
        for cancel_removal in [false, true] {
            let dir = TempDir::new();
            let mut seed = new_cache(strategy(dir.as_ref(), 0)).await;
            seed.put("FOO", b"old".as_slice()).await.unwrap();
            drop(seed);
            let mut cache = new_cache(strategy(dir.as_ref(), 10)).await;
            assert_eq!(cache.recover(|_| Some("foo")).await.unwrap(), 1);
            let pause = cache.strategy().writes.pause_commit(0);
            cancel(cache.put("foo", b"new".as_slice()), &pause).await;
            assert_eq!(counts(cache.strategy()), (0, 0, 3, 1));
            assert!(cache.exists("foo"));
            assert!(!cache_path(dir.as_ref(), "FOO").exists());
            // Cancel a removal while it waits for the promotion's rollback.
            if take {
                cancel_pending(cache.take("foo"));
            } else {
                cancel_pending(cache.delete("foo"));
            }
            assert!(cache.exists("foo"));
            pause.resume.notify_one();
            if cancel_removal {
                assert_eq!(cache.get("foo").await.unwrap(), b"old".as_slice());
                let pause = cache.strategy().writes.pause_commit(0);
                // Also cancel after the removal itself changed the disk.
                if take {
                    cancel(cache.take("foo"), &pause).await;
                } else {
                    cancel(cache.delete("foo"), &pause).await;
                }
                assert!(cache.exists("foo"));
                assert_eq!(counts(cache.strategy()), (0, 0, 3, 1));
                pause.resume.notify_one();
                cache.put("foo", b"again".as_slice()).await.unwrap();
                assert_eq!(cache.take("foo").await.unwrap(), b"again");
            } else if take {
                assert_eq!(cache.take("foo").await.unwrap(), b"old");
            } else {
                cache.delete("foo").await.unwrap();
            }
            assert_eq!(counts(cache.strategy()), (0, 0, 0, 0));
            for key in ["foo", "FOO"] {
                assert!(!cache_path(dir.as_ref(), key).exists());
            }
            drop(cache);
            let mut cache = new_cache(strategy(dir.as_ref(), 10)).await;
            assert_eq!(cache.recover(|key| Some(key.to_owned())).await.unwrap(), 0);
        }
    }
}

#[tokio::test]
async fn failed_rollback_only_blocks_its_own_key() {
    let dir = TempDir::new();
    let mut seed = new_cache(strategy(dir.as_ref(), 0)).await;
    for key in ["b", "c"] {
        seed.put(key, b"old".as_slice()).await.unwrap();
    }
    drop(seed);
    let mut cache = new_cache(strategy(dir.as_ref(), 16)).await;
    cache.recover(|key| Some(key.to_owned())).await.unwrap();
    let mut pauses = Vec::new();
    for key in ["b", "c"] {
        let pause = cache.strategy().writes.pause_commit(0);
        cancel(cache.put(key.to_owned(), b"new".as_slice()), &pause).await;
        pauses.push(pause);
    }
    assert_eq!(counts(cache.strategy()), (0, 0, 6, 2));
    let blocked = cache_path(dir.as_ref(), "b");
    fs::create_dir(&blocked).unwrap();
    for pause in pauses {
        pause.resume.notify_one();
    }
    assert_eq!(cache.take("c".to_owned()).await.unwrap(), b"old");
    for _ in 0..2 {
        assert!(cache.get("b".to_owned()).await.is_err());
        assert!(cache.take("b".to_owned()).await.is_err());
        assert!(cache.exists("b".to_owned()));
        cache.put("a".to_owned(), b"a".as_slice()).await.unwrap();
        cache
            .put("a".to_owned(), b"updated".as_slice())
            .await
            .unwrap();
        assert_eq!(cache.take("a".to_owned()).await.unwrap(), b"updated");
        cache.put("a".to_owned(), b"a".as_slice()).await.unwrap();
        cache.delete("a".to_owned()).await.unwrap();
        cache.put("disk".to_owned(), vec![1; 20]).await.unwrap();
        cache.put("disk".to_owned(), vec![2; 21]).await.unwrap();
        assert_eq!(cache.take("disk".to_owned()).await.unwrap(), vec![2; 21]);
        assert_eq!(counts(cache.strategy()), (0, 0, 3, 1));
    }
    fs::remove_dir(&blocked).unwrap();
    assert_eq!(cache.get("b".to_owned()).await.unwrap(), b"old".as_slice());
    cache
        .put("b".to_owned(), b"retry".as_slice())
        .await
        .unwrap();
    cache.delete("b".to_owned()).await.unwrap();
    assert_eq!(counts(cache.strategy()), (0, 0, 0, 0));
    drop(cache);
    let mut cache = new_cache(strategy(dir.as_ref(), 16)).await;
    assert_eq!(cache.recover(|key| Some(key.to_owned())).await.unwrap(), 0);
}
