use std::path::{Path, PathBuf};

use crate::Result;

#[cfg(feature = "rt_async-std_1")]
use async_std::fs::File;
#[cfg(any(
    feature = "blocking",
    all(
        feature = "implicit-blocking",
        not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
    )
))]
use std::fs::File;
#[cfg(feature = "rt_tokio_1")]
use tokio::fs::File;

/// A sequential file reader using the selected runtime.
pub struct Reader(File);

impl Reader {
    pub async fn open(path: &Path) -> Result<Self> {
        #[cfg(any(
            feature = "blocking",
            all(
                feature = "implicit-blocking",
                not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
            )
        ))]
        let file = File::open(path)?;
        #[cfg(any(feature = "rt_tokio_1", feature = "rt_async-std_1"))]
        let file = File::open(path).await?;
        Ok(Self(file))
    }

    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        #[cfg(any(
            feature = "blocking",
            all(
                feature = "implicit-blocking",
                not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
            )
        ))]
        {
            use std::io::Read;
            self.0.read_exact(buf)?;
        }
        #[cfg(feature = "rt_tokio_1")]
        {
            use tokio::io::AsyncReadExt;
            self.0.read_exact(buf).await?;
        }
        #[cfg(feature = "rt_async-std_1")]
        {
            use async_std::io::ReadExt;
            self.0.read_exact(buf).await?;
        }
        Ok(())
    }

    /// Read up to `limit` bytes into their final buffer. Untrusted lengths use
    /// no capacity hint, so a truncated file cannot force a large allocation.
    pub async fn read(&mut self, limit: u64, byte_len: Option<usize>) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(byte_len.unwrap_or(0));
        #[cfg(any(
            feature = "blocking",
            all(
                feature = "implicit-blocking",
                not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
            )
        ))]
        {
            use std::io::Read;
            (&mut self.0).take(limit).read_to_end(&mut buf)?;
        }
        #[cfg(feature = "rt_tokio_1")]
        {
            use tokio::io::AsyncReadExt;
            (&mut self.0).take(limit).read_to_end(&mut buf).await?;
        }
        #[cfg(feature = "rt_async-std_1")]
        {
            use async_std::io::ReadExt;
            (&mut self.0).take(limit).read_to_end(&mut buf).await?;
        }
        Ok(buf)
    }
}

pub async fn create_dir(path: impl AsRef<Path>) -> Result<()> {
    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
        )
    ))]
    {
        use std::fs::create_dir_all;
        Ok(create_dir_all(&path)?)
    }
    #[cfg(feature = "rt_tokio_1")]
    {
        use tokio::fs::create_dir_all;
        Ok(create_dir_all(&path).await?)
    }
    #[cfg(feature = "rt_async-std_1")]
    {
        use async_std::fs::create_dir_all;
        Ok(create_dir_all(path.as_ref()).await?)
    }
}

pub async fn read(path: impl AsRef<Path>, byte_len: Option<usize>) -> Result<Vec<u8>> {
    Reader::open(path.as_ref())
        .await?
        .read(u64::MAX, byte_len)
        .await
}

/// Write borrowed slices in order without concatenating them into a buffer.
pub async fn write_parts(path: impl AsRef<Path>, parts: &[&[u8]]) -> Result<()> {
    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
        )
    ))]
    {
        use std::{fs::File, io::Write};
        let mut file = File::create(path)?;
        for part in parts {
            file.write_all(part)?;
        }
        file.sync_data()?;
    }

    #[cfg(feature = "rt_tokio_1")]
    {
        use tokio::{fs::File, io::AsyncWriteExt};

        let mut file = File::create(path).await?;
        for part in parts {
            file.write_all(part).await?;
        }
        file.sync_data().await?;
    }

    #[cfg(feature = "rt_async-std_1")]
    {
        use async_std::{fs::File, io::WriteExt};

        let mut file = File::create(path.as_ref()).await?;
        for part in parts {
            file.write_all(part).await?;
        }
        file.sync_data().await?;
    }

    Ok(())
}

pub async fn delete(path: impl AsRef<Path>) -> Result<()> {
    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
        )
    ))]
    {
        Ok(std::fs::remove_file(path)?)
    }
    #[cfg(feature = "rt_tokio_1")]
    {
        Ok(tokio::fs::remove_file(path).await?)
    }
    #[cfg(feature = "rt_async-std_1")]
    {
        Ok(async_std::fs::remove_file(path.as_ref()).await?)
    }
}

pub async fn rename(from: &Path, to: &Path) -> Result<()> {
    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
        )
    ))]
    {
        Ok(std::fs::rename(from, to)?)
    }
    #[cfg(feature = "rt_tokio_1")]
    {
        Ok(tokio::fs::rename(from, to).await?)
    }
    #[cfg(feature = "rt_async-std_1")]
    {
        Ok(async_std::fs::rename(from, to).await?)
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
