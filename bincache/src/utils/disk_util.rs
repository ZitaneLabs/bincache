use std::path::{Path, PathBuf};

use crate::Result;

#[cfg(feature = "rt_async-std_1")]
use async_std::{
    fs,
    io::{ReadExt, WriteExt},
};
#[cfg(any(
    feature = "blocking",
    all(
        feature = "implicit-blocking",
        not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
    )
))]
use std::{
    fs,
    io::{Read, Write},
};
#[cfg(feature = "rt_tokio_1")]
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

// The runtime file APIs share their operations; only awaiting them differs.
macro_rules! file_io {
    ($operation:expr) => {{
        #[cfg(any(feature = "rt_tokio_1", feature = "rt_async-std_1"))]
        {
            $operation.await
        }
        #[cfg(any(
            feature = "blocking",
            all(
                feature = "implicit-blocking",
                not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
            )
        ))]
        {
            $operation
        }
    }};
}

/// A sequential file reader using the selected runtime.
pub struct Reader(fs::File);

impl Reader {
    pub async fn open(path: &Path) -> Result<Self> {
        #[cfg(test)]
        crate::utils::test::check_io(path, std::io::ErrorKind::PermissionDenied)?;
        Ok(Self(file_io!(fs::File::open(path))?))
    }

    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        file_io!(self.0.read_exact(buf))?;
        Ok(())
    }

    /// Read up to `limit` bytes into their final buffer. Untrusted lengths use
    /// no capacity hint, so a truncated file cannot force a large allocation.
    pub async fn read(&mut self, limit: u64, byte_len: Option<usize>) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(byte_len.unwrap_or(0));
        file_io!((&mut self.0).take(limit).read_to_end(&mut buf))?;
        Ok(buf)
    }
}

pub async fn create_dir(path: impl AsRef<Path>) -> Result<()> {
    Ok(file_io!(fs::create_dir_all(path.as_ref()))?)
}

pub async fn read(path: impl AsRef<Path>, byte_len: Option<usize>) -> Result<Vec<u8>> {
    Reader::open(path.as_ref())
        .await?
        .read(u64::MAX, byte_len)
        .await
}

/// Write borrowed slices in order without concatenating them into a buffer.
pub async fn write_parts(path: impl AsRef<Path>, parts: &[&[u8]]) -> Result<()> {
    let mut file = file_io!(fs::File::create(path.as_ref()))?;
    for part in parts {
        file_io!(file.write_all(part))?;
    }
    file_io!(file.sync_data())?;
    Ok(())
}

pub async fn delete(path: impl AsRef<Path>) -> Result<()> {
    Ok(file_io!(fs::remove_file(path.as_ref()))?)
}

pub async fn rename(from: &Path, to: &Path) -> Result<()> {
    Ok(file_io!(fs::rename(from, to))?)
}

/// Unlike a general rename, a cache deletion must never move a directory.
pub async fn rename_file(from: &Path, to: &Path) -> Result<()> {
    if !file_io!(fs::symlink_metadata(from))?.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    }
    rename(from, to).await
}

pub async fn copy(from: &Path, to: &Path) -> Result<()> {
    #[cfg(test)]
    crate::utils::test::check_io(from, std::io::ErrorKind::StorageFull)?;
    file_io!(fs::copy(from, to))?;
    Ok(())
}

/// Best-effort cleanup of a unique, non-authoritative transaction backup.
pub fn cleanup(path: PathBuf) {
    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1"))
        )
    ))]
    {
        _ = std::fs::remove_file(path);
    }
    #[cfg(feature = "rt_tokio_1")]
    {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                _ = delete(path).await;
            });
        }
    }
    #[cfg(feature = "rt_async-std_1")]
    {
        async_std::task::spawn(async move {
            _ = delete(path).await;
        });
    }
}

/// List regular files without following symlinks; skip unreadable entries.
pub async fn files(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
        )
    ))]
    {
        for entry in std::fs::read_dir(path)?.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                files.push(entry.path());
            }
        }
    }
    #[cfg(feature = "rt_tokio_1")]
    {
        let mut entries = tokio::fs::read_dir(path).await?;
        while let Some(entry) = entries.next_entry().await.transpose() {
            let Ok(entry) = entry else { continue };
            if entry.file_type().await.is_ok_and(|kind| kind.is_file()) {
                files.push(entry.path());
            }
        }
    }
    #[cfg(feature = "rt_async-std_1")]
    {
        use async_std::stream::StreamExt;
        let mut entries = async_std::fs::read_dir(path).await?;
        while let Some(entry) = entries.next().await {
            let Ok(entry) = entry else { continue };
            if entry.file_type().await.is_ok_and(|kind| kind.is_file()) {
                files.push(entry.path().into());
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::files;
    use crate::{async_test, utils::test::TempDir};

    async_test! {
        async fn test_files_skips_non_files() {
            let dir = TempDir::new();
            let first = dir.as_ref().join("first");
            let last = dir.as_ref().join("last");
            std::fs::write(&first, b"first").unwrap();
            std::fs::create_dir(dir.as_ref().join("directory")).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(dir.as_ref().join("missing"), dir.as_ref().join("broken-link")).unwrap();
            std::fs::write(&last, b"last").unwrap();

            let mut found = files(dir.as_ref()).await.unwrap();
            found.sort();
            assert_eq!(found, vec![first, last]);
            // Failure to open the directory itself is still an error.
            assert!(files(&dir.as_ref().join("missing")).await.is_err());
        }
    }
}
