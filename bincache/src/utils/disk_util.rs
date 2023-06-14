use std::path::Path;

use crate::Result;

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
        Ok(create_dir_all(&path)?)
    }
    #[cfg(feature = "rt_async-std_1")]
    {
        use async_std::fs::create_dir_all;
        Ok(create_dir_all(&path)?)
    }
}

pub async fn read(path: impl AsRef<Path>, byte_len: Option<usize>) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(byte_len.unwrap_or(0));

    #[cfg(any(
        feature = "blocking",
        all(
            feature = "implicit-blocking",
            not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")),
        )
    ))]
    {
        use std::{fs::File, io::Read};

        let mut file = File::open(path)?;
        file.read_to_end(&mut buf)?;
    }

    #[cfg(feature = "rt_tokio_1")]
    {
        use tokio::{fs::File, io::AsyncReadExt};

        let mut file = File::open(path).await?;
        file.read_to_end(&mut buf).await?;
    }

    #[cfg(feature = "rt_async-std_1")]
    {
        use async_std::{fs::File, io::ReadExt};

        let mut file = File::open(path.as_ref()).await?;
        file.read_to_end(&mut buf).await?;
    }

    Ok(buf)
}

pub async fn write(path: impl AsRef<Path>, value: &[u8]) -> Result<()> {
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
        file.write_all(value)?;
        file.sync_data()?;
    }

    #[cfg(feature = "rt_tokio_1")]
    {
        use tokio::{fs::File, io::AsyncWriteExt};

        let mut file = File::create(path).await?;
        file.write_all(value).await?;
        file.sync_data().await?;
    }

    #[cfg(feature = "rt_async-std_1")]
    {
        use async_std::{fs::File, io::WriteExt};

        let mut file = File::create(path.as_ref()).await?;
        file.write_all(value).await?;
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
