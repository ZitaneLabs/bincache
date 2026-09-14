use async_trait::async_trait;

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use crate::{
    CacheCapacity, DiskUtil, Result,
    traits::{CacheKey, CacheStrategy, RecoverableStrategy},
};

const LIMIT_KIND_BYTE: &str = "Stored bytes";
const LIMIT_KIND_ENTRY: &str = "Stored entries";
pub(crate) const FILE_MAGIC: &[u8; 8] = b"BINCACHE";

pub(crate) fn cache_path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(blake3::hash(key.as_bytes()).to_hex().as_str())
}

pub(crate) fn encode_entry(key: &str, value: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(FILE_MAGIC.len() + 8 + key.len() + value.len());
    encoded.extend_from_slice(FILE_MAGIC);
    encoded.extend_from_slice(&(key.len() as u64).to_le_bytes());
    encoded.extend_from_slice(key.as_bytes());
    encoded.extend_from_slice(value);
    encoded
}

pub(crate) fn decode_entry(encoded: &[u8]) -> Option<(&str, &[u8])> {
    if encoded.get(..8)? != FILE_MAGIC {
        return None;
    }
    let key_len = usize::try_from(u64::from_le_bytes(encoded.get(8..16)?.try_into().ok()?)).ok()?;
    let key_end = 16usize.checked_add(key_len)?;
    let key = std::str::from_utf8(encoded.get(16..key_end)?).ok()?;
    Some((key, encoded.get(key_end..)?))
}

pub(crate) async fn write_entry(path: &Path, key: &str, value: &[u8]) -> Result<()> {
    let tmp_path = path.with_extension("tmp");
    DiskUtil::write(&tmp_path, &encode_entry(key, value)).await?;
    if let Err(error) = std::fs::rename(&tmp_path, path) {
        _ = std::fs::remove_file(&tmp_path);
        return Err(error.into());
    }
    Ok(())
}

/// A stored file reference and its accounted payload length.
#[derive(Debug)]
pub struct Entry {
    path: PathBuf,
    byte_len: usize,
}

/// Disk-based cache strategy.
///
/// Stores one file per key, defaulting to unlimited storage in `./cache`.
/// Limits reject writes instead of evicting entries; bytes count stored payload
/// lengths, excluding headers and filesystem overhead. Reads load the whole file.
///
/// Writes sync file data before renaming a sibling temporary file into place;
/// the parent directory is not synced. This is not a crash-durability guarantee.
/// Dropping a cache leaves files on disk; recovering its index is explicit.
/// Use one live cache per dedicated directory. The format is an implementation
/// detail without cross-version compatibility guarantees.
/// See [persistence and recovery limitations](crate#semantics-and-guarantees).
#[derive(Debug)]
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
    /// Select a directory and independent stored-byte and entry-count limits.
    ///
    /// `None` is unlimited. Construction itself does no I/O; cache setup creates
    /// missing directories and can fail. It does not recover existing files.
    /// See the [disk/restart example](crate#disk-caching-and-recovery-after-restart).
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

impl Default for Disk {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("cache"),
            byte_limit: None,
            entry_limit: None,
            current_byte_count: 0,
            current_entry_count: 0,
        }
    }
}

#[async_trait]
impl CacheStrategy for Disk {
    type CacheEntry = Entry;

    async fn setup(&mut self) -> Result<()> {
        DiskUtil::create_dir(&self.cache_dir).await
    }

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
        let key = key.to_key();
        let path = cache_path(&self.cache_dir, &key);
        write_entry(&path, &key, value.as_ref()).await?;

        // Increment limits
        self.current_byte_count += byte_len;
        self.current_entry_count += 1;

        Ok(Entry { path, byte_len })
    }

    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        let encoded = DiskUtil::read(&entry.path, None).await?;
        let (_, value) = decode_entry(&encoded).ok_or_else(|| crate::Error::Custom {
            message: "Invalid disk cache entry".into(),
        })?;
        Ok(Cow::Owned(value.to_vec()))
    }

    async fn replace<'a, K, V>(
        &mut self,
        key: &K,
        entry: &mut Self::CacheEntry,
        value: V,
    ) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.len();
        let new_total = self.current_byte_count - entry.byte_len + byte_len;
        if self.byte_limit.is_some_and(|limit| new_total > limit) {
            return Err(crate::Error::LimitExceeded {
                limit_kind: LIMIT_KIND_BYTE.into(),
            });
        }

        let key = key.to_key();
        write_entry(&entry.path, &key, &value).await?;
        entry.byte_len = byte_len;
        self.current_byte_count = new_total;
        Ok(())
    }

    async fn take(&mut self, entry: Self::CacheEntry) -> Result<Vec<u8>> {
        let data = self.get(&entry).await?.into_owned();
        self.delete(entry).await?;

        Ok(data)
    }

    async fn delete(&mut self, entry: Self::CacheEntry) -> Result<()> {
        DiskUtil::delete(&entry.path).await?;

        // Decrement limits
        self.current_byte_count -= entry.byte_len;
        self.current_entry_count -= 1;

        Ok(())
    }

    fn get_cache_capacity(&self) -> Option<CacheCapacity> {
        self.byte_limit
            .map(|byte_limit| CacheCapacity::new(byte_limit, self.current_byte_count))
    }
}

#[async_trait]
impl RecoverableStrategy for Disk {
    async fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
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
            let Some(file_name) = source.file_name() else {
                return;
            };
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

            let buf = DiskUtil::read(&path, None).await?;
            let (key_str, value) = if let Some(decoded) = decode_entry(&buf) {
                decoded
            } else if buf.starts_with(FILE_MAGIC) {
                move_to_lost_found(&path);
                continue;
            } else {
                let Some(key_str) = path.file_name().and_then(|name| name.to_str()) else {
                    move_to_lost_found(&path);
                    continue;
                };
                (key_str, buf.as_slice())
            };
            let Some(key) = recover_key(key_str) else {
                move_to_lost_found(&path);
                continue;
            };

            // Increment limits
            self.current_byte_count += value.len();
            self.current_entry_count += 1;

            // Push entry
            entries.push((
                key,
                Entry {
                    path,
                    byte_len: value.len(),
                },
            ));
        }

        // Return recovered entries
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Disk, LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY, cache_path};
    use crate::{Cache, Error, NO_COMPRESSION, async_test, utils::test::TempDir};

    async_test! {
        async fn test_default() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();

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

        async fn test_strategy_with_byte_limit() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), Some(6), None), NO_COMPRESSION).await.unwrap();

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

            assert!(matches!(
                cache.put("baz", baz_data).await,
                Err(Error::LimitExceeded { limit_kind }) if limit_kind == LIMIT_KIND_BYTE
            ));
        }

        async fn test_strategy_with_entry_limit() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, Some(2)), NO_COMPRESSION).await.unwrap();

            cache.put("foo", b"foo".to_vec()).await.unwrap();
            cache.put("bar", b"bar".to_vec()).await.unwrap();

            assert_eq!(cache.get("foo").await.unwrap(), b"foo".as_slice());
            assert_eq!(cache.get("bar").await.unwrap(), b"bar".as_slice());

            assert!(matches!(
                cache.put("baz", b"baz".to_vec()).await,
                Err(Error::LimitExceeded { limit_kind }) if limit_kind == LIMIT_KIND_ENTRY
            ));
        }

        async fn test_recovery() {
            let temp_dir = TempDir::new();

            // populate cache
            {
                let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();

                cache.put("foo", b"foo".to_vec()).await.unwrap();
                cache.put("bar", b"bar".to_vec()).await.unwrap();
            }

            // recover cache
            {
                let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();
                let recovered_items = cache
                    .recover(|k| Some(k.to_string()))
                    .await
                    .expect("Failed to recover");

                assert_eq!(recovered_items, 2);
                assert_eq!(cache.strategy().current_byte_count, 6);
                assert_eq!(cache.strategy().current_entry_count, 2);
            }
        }

        async fn test_safe_keys_and_recovery() {
            let temp_dir = TempDir::new();
            let escaped = temp_dir.as_ref().with_extension("escaped");
            let traversal = format!("../{}", escaped.file_name().unwrap().to_string_lossy());
            let keys = [
                "normal".to_owned(),
                "foo/bar".to_owned(),
                r"foo\bar".to_owned(),
                traversal,
                escaped.to_string_lossy().into_owned(),
            ];

            {
                let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();
                for (index, key) in keys.iter().enumerate() {
                    cache.put(key.clone(), vec![index as u8]).await.unwrap();
                }
                assert_eq!(fs::read_dir(temp_dir.as_ref()).unwrap().count(), keys.len());
                assert!(!escaped.exists());
                assert!(!temp_dir.as_ref().join("foo").exists());
                assert!(fs::read_dir(temp_dir.as_ref()).unwrap().all(|entry| {
                    let path = entry.unwrap().path();
                    path.parent() == Some(temp_dir.as_ref()) && path.is_file()
                }));
            }

            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();
            assert_eq!(cache.recover(|key| Some(key.to_owned())).await.unwrap(), keys.len());
            for (index, key) in keys.iter().enumerate() {
                assert_eq!(cache.get(key.clone()).await.unwrap().as_ref(), &[index as u8]);
            }
        }

        async fn test_replace_accounting_capacity_and_failure() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), Some(100), None), NO_COMPRESSION).await.unwrap();
            cache.put("foo", vec![1; 100]).await.unwrap();
            cache.put("foo", vec![2; 50]).await.unwrap();
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().current_byte_count, 50);
            assert_eq!(cache.strategy().current_entry_count, 1);
            cache.put("foo", vec![5; 75]).await.unwrap();
            assert_eq!(cache.strategy().current_byte_count, 75);
            cache.put("foo", vec![2; 50]).await.unwrap();
            cache.put("bar", vec![3; 50]).await.unwrap();
            assert!(cache.put("foo", vec![4; 51]).await.is_err());
            assert_eq!(cache.get("foo").await.unwrap(), vec![2; 50]);
            assert_eq!(cache.strategy().current_byte_count, 100);
            assert_eq!(cache.strategy().current_entry_count, 2);
        }

        async fn test_failed_disk_write_keeps_previous_entry() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();
            cache.put("foo", b"old".to_vec()).await.unwrap();
            fs::create_dir(cache_path(temp_dir.as_ref(), "foo").with_extension("tmp")).unwrap();

            assert!(cache.put("foo", b"new".to_vec()).await.is_err());
            assert_eq!(cache.get("foo").await.unwrap(), b"old".as_slice());
            assert_eq!(cache.strategy().current_byte_count, 3);
            assert_eq!(cache.strategy().current_entry_count, 1);
        }
    }
}
