use super::Limits;
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
const FILE_MAGIC: &[u8; 8] = b"BINCACHE";
pub(crate) const FORMAT_DIR: &str = ".bincache-v1";

pub(crate) fn cache_path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir
        .join(FORMAT_DIR)
        .join(blake3::hash(key.as_bytes()).to_hex().as_str())
}

#[cfg(test)]
pub(crate) fn encode_entry(key: &str, value: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(FILE_MAGIC.len() + 8 + key.len() + value.len());
    encoded.extend_from_slice(FILE_MAGIC);
    encoded.extend_from_slice(&(key.len() as u64).to_le_bytes());
    encoded.extend_from_slice(key.as_bytes());
    encoded.extend_from_slice(value);
    encoded
}

fn decode_key_len(header: &[u8]) -> Option<usize> {
    if header.get(..8)? != FILE_MAGIC {
        return None;
    }
    usize::try_from(u64::from_le_bytes(header.get(8..16)?.try_into().ok()?)).ok()
}

pub(crate) fn decode_entry(encoded: &[u8]) -> Option<(&str, &[u8])> {
    let key_len = decode_key_len(encoded)?;
    let key_end = 16usize.checked_add(key_len)?;
    let key = std::str::from_utf8(encoded.get(16..key_end)?).ok()?;
    Some((key, encoded.get(key_end..)?))
}

fn invalid_entry() -> crate::Error {
    crate::Error::Custom {
        message: "Invalid disk cache entry".into(),
    }
}

/// A disk entry, shared by the disk and hybrid strategies.
#[derive(Debug)]
pub struct Entry {
    pub(crate) path: PathBuf,
    // Retain the serialized key that determined the path, even if recovery
    // maps it to a different in-memory key.
    key: String,
    pub(crate) byte_len: usize,
}

impl Entry {
    pub(crate) fn new(cache_dir: &Path, key: &str, byte_len: usize) -> Self {
        Self {
            path: cache_path(cache_dir, key),
            key: key.to_owned(),
            byte_len,
        }
    }

    pub(crate) async fn read(&self) -> Result<Vec<u8>> {
        let mut file = DiskUtil::Reader::open(&self.path).await?;
        let mut header = [0; 16];
        file.read_exact(&mut header).await?;
        let key_len = decode_key_len(&header).ok_or_else(invalid_entry)?;
        let key = file.read(key_len as u64, None).await?;
        if key.len() != key_len || std::str::from_utf8(&key).is_err() {
            return Err(invalid_entry());
        }
        drop(key);
        // The payload is read directly into its final, preallocated buffer.
        file.read(u64::MAX, Some(self.byte_len)).await
    }

    pub(super) async fn write(&self, value: &[u8]) -> Result<()> {
        let temporary = self.path.with_extension("tmp");
        let key_len = (self.key.len() as u64).to_le_bytes();
        let result = async {
            DiskUtil::write_parts(
                &temporary,
                &[b"BINCACHE", &key_len, self.key.as_bytes(), value],
            )
            .await?;
            DiskUtil::rename(&temporary, &self.path).await
        }
        .await;
        if result.is_err() {
            _ = DiskUtil::delete(&temporary).await;
        }
        result
    }
}

pub(crate) async fn recover_entries<K, F>(
    cache_dir: &Path,
    recover_key: F,
) -> Result<Vec<(K, Entry)>>
where
    F: Fn(&str) -> Option<K>,
{
    let mut entries = Vec::new();
    let dir = cache_dir.join(FORMAT_DIR);
    let lost_found = dir.join("lost+found");
    DiskUtil::create_dir(&lost_found).await?;
    for path in DiskUtil::files(&dir).await? {
        let recovered = if path.extension().is_some_and(|ext| ext == "tmp") {
            // A synced temporary file has not passed the rename commit point.
            None
        } else {
            // An unreadable record may be valid; leave it for a later recovery.
            let Ok(data) = DiskUtil::read(&path, None).await else {
                continue;
            };
            decode_entry(&data).and_then(|(key, value)| {
                let entry = Entry::new(cache_dir, key, value.len());
                (entry.path == path).then_some(())?;
                Some((recover_key(key)?, entry))
            })
        };
        if let Some(entry) = recovered {
            entries.push(entry);
        } else if let Some(name) = path.file_name() {
            _ = DiskUtil::rename(&path, &lost_found.join(name)).await;
        }
    }
    Ok(entries)
}

/// Disk-based cache strategy.
///
/// This strategy stores entries on disk. It can be configured to limit the
/// number of bytes and/or entries that can be stored.
#[derive(Debug)]
pub struct Disk {
    /// The directory where entries are stored.
    cache_dir: PathBuf,
    limits: Limits,
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
            limits: Limits::new(byte_limit, entry_limit),
        }
    }
}

impl Default for Disk {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("cache"),
            limits: Limits::default(),
        }
    }
}

#[async_trait]
impl CacheStrategy for Disk {
    type CacheEntry = Entry;

    async fn setup(&mut self) -> Result<()> {
        DiskUtil::create_dir(self.cache_dir.join(FORMAT_DIR)).await
    }

    async fn put<'a, K, V>(&mut self, key: &K, value: V) -> Result<Self::CacheEntry>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.as_ref().len();

        self.limits
            .check(byte_len, None, [LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY])?;

        // Write to disk
        let key = key.to_key();
        let entry = Entry::new(&self.cache_dir, &key, byte_len);
        entry.write(value.as_ref()).await?;

        self.limits.add(byte_len);

        Ok(entry)
    }

    async fn get<'a>(&self, entry: &'a Self::CacheEntry) -> Result<Cow<'a, [u8]>> {
        Ok(Cow::Owned(entry.read().await?))
    }

    async fn replace<'a, K, V>(
        &mut self,
        _key: &K,
        entry: &mut Self::CacheEntry,
        value: V,
    ) -> Result<()>
    where
        K: CacheKey + Sync + Send,
        V: Into<Cow<'a, [u8]>> + Send,
    {
        let value = value.into();
        let byte_len = value.len();
        let new_total = self.limits.replacement_size(byte_len, entry.byte_len)?;

        entry.write(&value).await?;
        entry.byte_len = byte_len;
        self.limits.current_byte_count = new_total;
        Ok(())
    }

    async fn take(&mut self, entry: &mut Self::CacheEntry) -> Result<Vec<u8>> {
        let data = self.get(entry).await?.into_owned();
        self.delete(entry).await?;

        Ok(data)
    }

    async fn delete(&mut self, entry: &mut Self::CacheEntry) -> Result<()> {
        DiskUtil::delete(&entry.path).await?;

        self.limits.remove(entry.byte_len);

        Ok(())
    }

    fn get_cache_capacity(&self) -> Option<CacheCapacity> {
        self.limits.capacity()
    }
}

#[async_trait]
impl RecoverableStrategy for Disk {
    async fn recover<K, F>(&mut self, recover_key: F) -> Result<Vec<(K, Self::CacheEntry)>>
    where
        K: Send,
        F: Fn(&str) -> Option<K> + Send,
    {
        let entries = recover_entries(&self.cache_dir, recover_key).await?;
        for (_, entry) in &entries {
            self.limits.add(entry.byte_len);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use crate::strategies::test_helpers;
    use std::fs;

    use super::{Disk, Entry, FORMAT_DIR, LIMIT_KIND_BYTE, LIMIT_KIND_ENTRY, encode_entry};
    use crate::{Cache, NO_COMPRESSION, async_test, utils::test::TempDir};

    async_test! {
        async fn test_read_preallocates_only_payload() {
            let dir = TempDir::new();
            let key = "🔑".repeat(16 * 1024);
            let value = vec![0xa5; 100];
            let mut entry = Entry::new(dir.as_ref(), &key, value.len());
            crate::DiskUtil::create_dir(dir.as_ref().join(FORMAT_DIR)).await.unwrap();
            entry.write(&value).await.unwrap();
            // A hint larger than the payload demonstrates that the reader uses
            // byte_len, while a much larger key must not inflate that buffer.
            entry.byte_len = 4096;
            let read = entry.read().await.unwrap();
            assert_eq!(read, value);
            assert!(read.capacity() >= entry.byte_len);
            assert!(read.capacity() < key.len());
        }

        async fn test_read_rejects_invalid_headers_and_keys() {
            let dir = TempDir::new();
            let mut oversized = b"BINCACHE".to_vec();
            oversized.extend_from_slice(&u64::MAX.to_le_bytes());
            let mut invalid_utf8 = encode_entry("a", b"value");
            invalid_utf8[16] = 0xff;
            let mut truncated_key = encode_entry("key", b"");
            truncated_key.pop();
            let mut bad_magic = encode_entry("key", b"value");
            bad_magic[0] = b'?';
            let entry = Entry::new(dir.as_ref(), "key", 0);
            crate::DiskUtil::create_dir(dir.as_ref().join(FORMAT_DIR)).await.unwrap();
            for encoded in [b"BINCACHE".to_vec(), oversized, invalid_utf8, truncated_key, bad_magic] {
                fs::write(&entry.path, encoded).unwrap();
                assert!(entry.read().await.is_err());
            }
        }

        async fn test_segmented_write_round_trip() {
            let dir = TempDir::new();
            let mut disk = Disk::new(dir.as_ref(), None, None);
            crate::CacheStrategy::setup(&mut disk).await.unwrap();
            for (key, value) in [("", Vec::new()), ("a/🔑\\key", vec![0xa5; 2 * 1024 * 1024])] {
                let entry = Entry::new(dir.as_ref(), key, value.len());
                entry.write(&value).await.unwrap();
                assert_eq!(fs::read(&entry.path).unwrap(), encode_entry(key, &value));
                assert_eq!(entry.read().await.unwrap(), value);
            }
        }

        async fn test_default_strategy() {
            let dir = TempDir::new();
            test_helpers::basic(Disk::new(dir.as_ref(), None, None), |strategy| (strategy.limits.current_byte_count, strategy.limits.current_entry_count)).await;
        }

        async fn test_strategy_with_byte_limit() {
            let dir = TempDir::new();
            test_helpers::limit(Disk::new(dir.as_ref(), Some(6), None), LIMIT_KIND_BYTE).await;
        }

        async fn test_strategy_with_entry_limit() {
            let dir = TempDir::new();
            test_helpers::limit(Disk::new(dir.as_ref(), None, Some(2)), LIMIT_KIND_ENTRY).await;
        }

        async fn test_recovery() {
            let dir = TempDir::new();
            test_helpers::recovery(
                Disk::new(dir.as_ref(), None, None),
                Disk::new(dir.as_ref(), None, None),
                |strategy| (strategy.limits.current_byte_count, strategy.limits.current_entry_count),
                &["foo", "bar"], &["foo", "bar"],
            ).await;
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
                let entries_dir = temp_dir.as_ref().join(FORMAT_DIR);
                assert_eq!(fs::read_dir(&entries_dir).unwrap().count(), keys.len());
                assert!(!escaped.exists());
                assert!(!temp_dir.as_ref().join("foo").exists());
                assert!(fs::read_dir(&entries_dir).unwrap().all(|entry| {
                    let path = entry.unwrap().path();
                    path.parent() == Some(entries_dir.as_path()) && path.is_file()
                }));
            }

            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();
            assert_eq!(cache.recover(|key| Some(key.to_owned())).await.unwrap(), keys.len());
            for (index, key) in keys.iter().enumerate() {
                assert_eq!(cache.get(key.clone()).await.unwrap().as_ref(), &[index as u8]);
            }
        }

        async fn test_replace_accounting_capacity_and_failure() {
            let dir = TempDir::new();
            test_helpers::replacement(Disk::new(dir.as_ref(), Some(100), None), |strategy| (strategy.limits.current_byte_count, strategy.limits.current_entry_count)).await;
        }

        async fn test_failed_disk_write_keeps_previous_entry() {
            let temp_dir = TempDir::new();
            let mut cache = Cache::new(Disk::new(temp_dir.as_ref(), None, None), NO_COMPRESSION).await.unwrap();
            cache.put("foo", b"old".to_vec()).await.unwrap();
            let directory = temp_dir.as_ref().join(FORMAT_DIR);
            let saved = temp_dir.as_ref().join("saved");
            fs::rename(&directory, &saved).unwrap();
            fs::write(&directory, b"cannot stage inside a file").unwrap();

            assert!(cache.put("foo", b"new".to_vec()).await.is_err());
            assert!(cache.take("foo").await.is_err());
            assert!(cache.delete("foo").await.is_err());
            assert!(cache.exists("foo"));
            fs::remove_file(&directory).unwrap();
            fs::rename(&saved, &directory).unwrap();
            assert_eq!(cache.get("foo").await.unwrap(), b"old".as_slice());
            assert_eq!(cache.strategy().limits.current_byte_count, 3);
            assert_eq!(cache.strategy().limits.current_entry_count, 1);
        }

        async fn test_replace_normalized_recovered_key() {
            crate::strategies::recovery_tests::replaces_normalized_key(
                |path| Disk::new(path, None, Some(1)),
                |disk| (disk.limits.current_byte_count, disk.limits.current_entry_count),
            ).await;
        }

        async fn test_recovery_ignores_old_formats() {
            crate::strategies::recovery_tests::ignores_old_formats(
                |path| Disk::new(path, None, Some(1)),
                |disk| (disk.limits.current_byte_count, disk.limits.current_entry_count),
            ).await;
        }

        async fn test_interrupted_write_recovery() {
            crate::strategies::recovery_tests::interrupted(
                |path| Disk::new(path, None, None),
                |disk| (disk.limits.current_byte_count, disk.limits.current_entry_count),
            ).await;
        }
    }
}
