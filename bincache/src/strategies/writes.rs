use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use futures_util::lock::Mutex as AsyncMutex;

use crate::{DiskUtil, Result};

type Operation<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

struct StagedFile(PathBuf);

impl Drop for StagedFile {
    fn drop(&mut self) {
        DiskUtil::cleanup(self.0.clone());
    }
}

enum Phase {
    Applying(Operation<bool>),
    Reverting(Option<bool>, Operation<()>),
}

struct Transaction {
    path: PathBuf,
    backup: PathBuf,
    phase: Phase,
}

impl Transaction {
    fn revert(&self, original: Option<bool>) -> Operation<()> {
        let (path, backup) = (self.path.clone(), self.backup.clone());
        Box::pin(async move {
            match original {
                Some(true) => DiskUtil::rename(&backup, &path).await?,
                Some(false) => DiskUtil::delete(&path).await?,
                None => {} // The operation failed before changing the destination.
            }
            Ok(())
        })
    }

    async fn settle(&mut self) -> Result<()> {
        if let Phase::Applying(operation) = &mut self.phase {
            let original = operation.await.ok();
            self.phase = Phase::Reverting(original, self.revert(original));
        }
        if let Phase::Reverting(original, operation) = &mut self.phase {
            if let Err(error) = operation.await {
                let original = *original;
                self.phase = Phase::Reverting(original, self.revert(original));
                return Err(error);
            }
        }
        Ok(())
    }
}

/// Own filesystem commits beyond the lifetime of the caller. A change is acknowledged
/// only when its future returns; cancellation retains it for rollback before any
/// later access to the same path. Different paths remain independent.
#[derive(Default)]
pub(super) struct Writes {
    pending: Mutex<HashMap<PathBuf, Arc<AsyncMutex<Transaction>>>>,
    #[cfg(all(test, feature = "rt_tokio_1"))]
    pause: Mutex<Option<(usize, Arc<CommitPause>)>>,
}

fn temporary_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(all(test, feature = "rt_tokio_1"))]
#[derive(Default)]
pub(super) struct CommitPause {
    pub reached: tokio::sync::Notify,
    pub resume: tokio::sync::Notify,
}

impl std::fmt::Debug for Writes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writes")
            .field("pending", &self.pending.lock().unwrap().keys())
            .finish()
    }
}

impl Writes {
    #[cfg(all(test, feature = "rt_tokio_1"))]
    pub(super) fn pause_commit(&self, writes: usize) -> Arc<CommitPause> {
        let pause = Arc::new(CommitPause::default());
        *self.pause.lock().unwrap() = Some((writes, Arc::clone(&pause)));
        pause
    }
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.lock().unwrap().is_empty()
    }

    pub(super) fn pending_path(&self, path: PathBuf) -> Option<PathBuf> {
        self.pending
            .lock()
            .unwrap()
            .contains_key(&path)
            .then_some(path)
    }
    pub(super) async fn settle(&self, path: &Path) -> Result<()> {
        let transaction = self.pending.lock().unwrap().get(path).cloned();
        if let Some(transaction) = transaction {
            let mut transaction = transaction.lock().await;
            // Other readers may already have settled this transaction.
            if !self.pending.lock().unwrap().contains_key(path) {
                return Ok(());
            }
            transaction.settle().await?;
            self.pending.lock().unwrap().remove(path);
            DiskUtil::cleanup(transaction.backup.clone());
        }
        Ok(())
    }

    pub(super) async fn settle_all(&self) -> Result<()> {
        let paths: Vec<_> = self.pending.lock().unwrap().keys().cloned().collect();
        for path in paths {
            self.settle(&path).await?;
        }
        Ok(())
    }

    pub(super) async fn write(&self, path: &Path, key: &str, value: &[u8]) -> Result<()> {
        self.settle(path).await?;
        // Staging borrows the payload. If canceled, late runtime I/O can only
        // affect this unique .tmp, which recovery never treats as committed.
        let temporary = StagedFile(temporary_path(path));
        let key_len = (key.len() as u64).to_le_bytes();
        DiskUtil::write_parts(
            &temporary.0,
            &[b"BINCACHE", &key_len, key.as_bytes(), value],
        )
        .await?;
        self.commit(path, Some(temporary)).await
    }

    pub(super) async fn delete(&self, path: &Path) -> Result<()> {
        self.settle(path).await?;
        self.commit(path, None).await
    }

    async fn commit(&self, path: &Path, staged: Option<StagedFile>) -> Result<()> {
        let backup = temporary_path(path);
        let (destination, saved) = (path.to_owned(), backup.clone());
        #[cfg(all(test, feature = "rt_tokio_1"))]
        let pause = {
            let mut pause = self.pause.lock().unwrap();
            match pause.as_mut() {
                Some((0, _)) => pause.take().map(|(_, pause)| pause),
                Some((remaining, _)) => {
                    *remaining -= 1;
                    None
                }
                None => None,
            }
        };
        let operation = Box::pin(async move {
            let result = async {
                let Some(staged) = &staged else {
                    // Moving the original preserves rollback without allocating a copy.
                    DiskUtil::rename_file(&destination, &saved).await?;
                    return Ok(true);
                };
                let original = match DiskUtil::copy(&destination, &saved).await {
                    Ok(()) => true,
                    Err(crate::Error::IoError(error))
                        if error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        false
                    }
                    Err(error) => return Err(error),
                };
                DiskUtil::rename(&staged.0, &destination).await?;
                Ok(original)
            }
            .await;
            if result.is_err() {
                if let Some(staged) = staged {
                    _ = DiskUtil::delete(&staged.0).await;
                }
            }
            let original = result?;
            // Model a completed filesystem commit whose result has not yet
            // reached the caller. The transaction must survive that caller.
            #[cfg(all(test, feature = "rt_tokio_1"))]
            if let Some(pause) = pause {
                pause.reached.notify_one();
                pause.resume.notified().await;
            }
            Ok(original)
        });
        let transaction = Arc::new(AsyncMutex::new(Transaction {
            path: path.to_owned(),
            backup,
            phase: Phase::Applying(operation),
        }));
        self.pending
            .lock()
            .unwrap()
            .insert(path.to_owned(), Arc::clone(&transaction));
        let mut transaction = transaction.lock().await;
        let Phase::Applying(operation) = &mut transaction.phase else {
            unreachable!()
        };
        let result = operation.await.map(|_| ());
        // No await between acknowledgement and the caller's entry/accounting
        // update. Only non-authoritative backup cleanup remains.
        self.pending.lock().unwrap().remove(path);
        DiskUtil::cleanup(transaction.backup.clone());
        result
    }
}
