use async_trait::async_trait;

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use crate::{
    traits::{CacheKey, CacheStrategy, RecoverableStrategy},
    Result,
};

const LIMIT_KIND_BYTE: &str = "Stored bytes";
const LIMIT_KIND_ENTRY: &str = "Stored entries";

pub struct Entry {
    path: PathBuf,
    byte_len: usize,
}

/// Disk-based cache strategy.
///
/// This strategy stores entries on disk. It can be configured to limit the
/// number of bytes and/or entries that can be stored.
#[derive(Default, Debug)]
pub struct Disk {
    /// The directory where entries are stored.
    cache_dir: PathBuf,
    /// The maximum number of bytes that can be stored.
    byte_limit: Option<usize>,
    /// The maximum number of entries that can be stored.
    entry_limit: Option<usize>,
    /// The current number of bytes stored.
    current_byte_count: usize,
    /// The current number of entries stored.
    current_entry_count: usize,
}

impl Disk {
    /// Create a new disk cache strategy.
    pub fn new<'a>(
        cache_dir: impl Into<Cow<'a, Path>>,
        byte_limit: Option<usize>,
        entry_limit: Option<usize>,
    ) -> Self {
        Self {
            cache_dir: cache_dir.into().into_owned(),
            byte_limit,
            entry_limit,
            ..Default::default()
        }
    }
}

impl Disk {
    async fn read_from_disk(&self, entry: &Entry) -> Result<Vec<u8>> {
        #[cfg(feature = "blocking")]
        {
            use std::{fs::File, io::Read};

            let mut file = File::open(&entry.path)?;
            let mut buf = Vec::with_capacity(entry.byte_len);
            file.read_to_end(&mut buf)?;
            Ok(buf)
        }
        #[cfg(feature = "tokio_rt_1")]
        {
            use tokio::fs::File;
            use tokio::io::AsyncReadExt;

            let mut file = File::open(&entry.path).await?;
            let mut buf = Vec::with_capacity(entry.byte_len);
            file.read_to_end(&mut buf).await?;
            Ok(buf)
        }
    }

    async fn write_to_disk(&self, path: impl AsRef<Path>, value: &[u8]) -> Result<()> {
        #[cfg(feature = "blocking")]
        {
            use std::{fs::File, io::Write};

            let mut file = File::create(path)?;
            file.write_all(value)?;
            file.sync_data()?;
            Ok(())
        }
        #[cfg(feature = "tokio_rt_1")]
        {
            use tokio::fs::File;
            use tokio::io::AsyncWriteExt;

            let mut file = File::create(path).await?;
            file.write_all(value).await?;
            file.sync_data().await?;
            Ok(())
        }
    }

    async fn delete_from_disk(&self, entry: &Entry) -> Result<()> {
        #[cfg(feature = "blocking")]
        {
            Ok(std::fs::remove_file(&entry.path)?)
        }
        #[cfg(feature = "tokio_rt_1")]
        {
            Ok(tokio::fs::remove_file(&entry.path).await?)
        }
    }
}

#[async_trait]
impl CacheStrategy for Disk {
    type CacheEntry = Entry;

    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.as_ref().len();

        // Check if the byte limit has been reached.
        if let Some(byte_limit) = self.byte_limit {
            if self.current_byte_count + byte_len > byte_limit {
                return Err(crate::Error::LimitExceeded {
                    limit_kind: LIMIT_KIND_BYTE.into(),
                });
            }
        }

        // Check if entry limit has been reached.
        if let Some(entry_limit) = self.entry_limit {
            if self.current_entry_count + 1 > entry_limit {
                return Err(crate::Error::LimitExceeded {
                    limit_kind: LIMIT_KIND_ENTRY.into(),
                });
            }
        }

        // Write to disk
        let path = self.cache_dir.join(key.to_key());
        self.write_to_disk(&path, value.as_ref()).await?;

        // Increment limits
        self.current_byte_count += byte_len;
        self.current_entry_count += 1;

        Ok(Entry { path, byte_len })
    }

    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        self.read_from_disk(entry).await.map(Cow::Owned)
    }

    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        let data = self.read_from_disk(&entry).await?;
        self.delete(entry).await?;

        Ok(data)
    }

    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        self.delete_from_disk(&entry).await?;

        // Decrement limits
        self.current_byte_count -= entry.byte_len;
        self.current_entry_count -= 1;

        Ok(())
    }
}

#[async_trait]
impl RecoverableStrategy for Disk {
    async fn recover<K, F>(&mut self, mut recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        // Create the `lost+found` directory
        let lost_found_dir = self.cache_dir.join("lost+found");
        std::fs::create_dir_all(&lost_found_dir)?;

        // Closure to move files to the `lost+found` directory
        let move_to_lost_found = |source: &Path| {
            // We explcitly ignore any errors here, as we don't want to fail
            // the entire recovery process because of a single file.
            let Some(file_name) = source.file_name() else { return };
            let target_path = lost_found_dir.join(file_name);
            _ = std::fs::rename(source, target_path);
        };

        // Iterate over all files in the cache directory
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.cache_dir)?.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // If key recovery fails, we move the entry to the `lost+found` directory.
            let Some(key) = path.file_name().and_then(|p| p.to_str()).and_then(&mut recover_key) else {
                move_to_lost_found(&path);
                continue
            };

            // Read file
            let buf = {
                #[cfg(feature = "blocking")]
                {
                    use std::fs::File;
                    use std::io::Read;

                    let mut file = File::open(&path)?;
                    let mut buf = Vec::new();
                    file.read_to_end(&mut buf)?;
                    buf
                }
                #[cfg(feature = "tokio_rt_1")]
                {
                    use tokio::fs::File;
                    use tokio::io::AsyncReadExt;

                    let mut file = File::open(&path).await?;
                    let mut buf = Vec::new();
                    file.read_to_end(&mut buf).await?;
                    buf
                }
            };

            // Increment limits
            self.current_byte_count += buf.len();
            self.current_entry_count += 1;

            // Push entry
            entries.push((
                key,
                Entry {
                    path,
                    byte_len: buf.len(),
                },
            ));
        }

        // Return recovered entries
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::{Disk, LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY};
    use crate::{test_utils::TempDir, Cache, Error};

    #[cfg_attr(feature = "tokio_rt_1", tokio::test)]
    async fn test_default() {
        let temp_dir = TempDir::new();
        let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None));

        cache.put("foo", b"foo".to_vec()).await.unwrap();

        assert_eq!(cache.strategy().current_byte_count, 3);
        assert_eq!(cache.strategy().current_entry_count, 1);

        cache.put("bar", b"bar".to_vec()).await.unwrap();

        assert_eq!(cache.strategy().current_byte_count, 6);
        assert_eq!(cache.strategy().current_entry_count, 2);

        assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

        assert!(cache.get("baz").await.is_err());

        cache.delete("foo").await.unwrap();

        assert_eq!(cache.strategy().current_byte_count, 3);
        assert_eq!(cache.strategy().current_entry_count, 1);

        cache.delete("bar").await.unwrap();

        assert_eq!(cache.strategy().current_byte_count, 0);
        assert_eq!(cache.strategy().current_entry_count, 0);
    }

    #[cfg_attr(feature = "tokio_rt_1", tokio::test)]
    async fn test_strategy_with_byte_limit() {
        let temp_dir = TempDir::new();
        let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), Some(6), None));

        let foo_data = b"foo".to_vec();
        let bar_data = b"bar".to_vec();
        let baz_data = b"baz".to_vec();

        assert_eq!(foo_data.len(), 3);
        assert_eq!(bar_data.len(), 3);
        assert_eq!(baz_data.len(), 3);

        cache.put("foo", foo_data.clone()).await.unwrap();
        cache.put("bar", bar_data.clone()).await.unwrap();

        assert_eq!(cache.get("foo").await.unwrap(), foo_data.as_slice());
        assert_eq!(cache.get("bar").await.unwrap(), bar_data.as_slice());

        match cache.put("baz", baz_data).await {
            Err(err) => match err {
                Error::LimitExceeded { limit_kind } => {
                    assert_eq!(limit_kind, LIMIT_KIND_BYTE);
                }
                _ => panic!("Unexpected error: {:?}", err),
            },
            _ => (),
        }
    }

    #[cfg_attr(feature = "tokio_rt_1", tokio::test)]
    async fn test_strategy_with_entry_limit() {
        let temp_dir = TempDir::new();
        let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, Some(3)));

        cache.put("foo", b"foo".to_vec()).await.unwrap();
        cache.put("bar", b"bar".to_vec()).await.unwrap();

        assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
        assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

        match cache.put("baz", b"baz".to_vec()).await {
            Err(err) => match err {
                Error::LimitExceeded { limit_kind } => {
                    assert_eq!(limit_kind, LIMIT_KIND_ENTRY);
                }
                _ => panic!("Unexpected error: {:?}", err),
            },
            _ => (),
        }
    }

    #[cfg_attr(feature = "tokio_rt_1", tokio::test)]
    async fn test_recovery() {
        let temp_dir = TempDir::new();

        // populate cache
        {
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None));

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();
        }

        // recover cache
        {
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None));
            let recovered_items = cache
                .recover(|k| Some(k.to_string()))
                .await
                .expect("Failed to recover");

            assert_eq!(recovered_items, 2);
            assert_eq!(cache.strategy().current_byte_count, 6);
            assert_eq!(cache.strategy().current_entry_count, 2);
        }
    }
}
