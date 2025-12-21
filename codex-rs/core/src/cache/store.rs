use crate::cache::LOG_TARGET;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub key: String,
    pub value: Vec<u8>,
    pub ttl: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStoreStats {
    pub entries: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStorePutOutcome {
    pub evicted: usize,
}

pub trait CacheStore: Send + Sync {
    fn get(&self, key: &str) -> std::io::Result<Option<CacheEntry>>;
    fn put(&self, entry: CacheEntry) -> std::io::Result<CacheStorePutOutcome>;
    fn remove(&self, key: &str) -> std::io::Result<()>;
    fn clear(&self) -> std::io::Result<()>;
    fn stats(&self) -> std::io::Result<CacheStoreStats>;
}

#[derive(Debug)]
pub struct DiskCacheStore {
    inner: Mutex<CacheIndex>,
    index_path: PathBuf,
    entries_path: PathBuf,
    max_bytes: u64,
}

impl DiskCacheStore {
    pub fn new(cache_dir: &Path, max_bytes: u64) -> std::io::Result<Self> {
        std::fs::create_dir_all(cache_dir)?;
        let entries_path = cache_dir.join("entries");
        std::fs::create_dir_all(&entries_path)?;
        let index_path = cache_dir.join("index.json");
        let mut index = Self::load_index(&index_path).unwrap_or_else(|err| {
            warn!(
                target: LOG_TARGET,
                "failed to load cache index: {err}"
            );
            CacheIndex::default()
        });
        index.prune_expired(&entries_path)?;
        index.recalculate_bytes(&entries_path)?;
        Ok(Self {
            inner: Mutex::new(index),
            index_path,
            entries_path,
            max_bytes,
        })
    }

    fn load_index(path: &Path) -> std::io::Result<CacheIndex> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CacheIndex::default());
            }
            Err(err) => return Err(err),
        };
        let index = serde_json::from_slice(&bytes).map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{err}"))
        })?;
        Ok(index)
    }

    fn persist_index(&self, index: &CacheIndex) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(index).map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{err}"))
        })?;
        let tmp_path = self.index_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, bytes)?;
        std::fs::rename(tmp_path, &self.index_path)?;
        Ok(())
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.entries_path.join(key)
    }
}

impl CacheStore for DiskCacheStore {
    fn get(&self, key: &str) -> std::io::Result<Option<CacheEntry>> {
        let mut index = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "cache lock poisoned"))?;
        let (ttl_secs, value) = {
            let entry = match index.entries.get_mut(key) {
                Some(entry) => entry,
                None => return Ok(None),
            };
            if entry.is_expired() {
                let _ = index.remove_entry(key, &self.entries_path);
                self.persist_index(&index)?;
                return Ok(None);
            }
            let entry_path = self.entry_path(key);
            let value = match std::fs::read(&entry_path) {
                Ok(value) => value,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let _ = index.remove_entry(key, &self.entries_path);
                    self.persist_index(&index)?;
                    return Ok(None);
                }
                Err(err) => return Err(err),
            };
            entry.last_access_epoch = now_epoch_secs();
            (entry.ttl_secs, value)
        };
        self.persist_index(&index)?;
        Ok(Some(CacheEntry {
            key: key.to_string(),
            value,
            ttl: Duration::from_secs(ttl_secs),
        }))
    }

    fn put(&self, entry: CacheEntry) -> std::io::Result<CacheStorePutOutcome> {
        if self.max_bytes == 0 {
            return Ok(CacheStorePutOutcome { evicted: 0 });
        }
        let mut index = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "cache lock poisoned"))?;
        let size_bytes = entry.value.len() as u64;
        if size_bytes > self.max_bytes {
            return Ok(CacheStorePutOutcome { evicted: 0 });
        }
        if index.entries.contains_key(&entry.key) {
            index.remove_entry(&entry.key, &self.entries_path)?;
        }
        let mut evicted = 0;
        while index.total_bytes + size_bytes > self.max_bytes {
            let Some((oldest_key, _)) = index.oldest_entry() else {
                break;
            };
            index.remove_entry(&oldest_key, &self.entries_path)?;
            evicted += 1;
        }
        let entry_path = self.entry_path(&entry.key);
        std::fs::write(&entry_path, &entry.value)?;
        index.total_bytes += size_bytes;
        index.entries.insert(
            entry.key.clone(),
            CacheIndexEntry {
                size_bytes,
                inserted_epoch: now_epoch_secs(),
                last_access_epoch: now_epoch_secs(),
                ttl_secs: entry.ttl.as_secs(),
            },
        );
        self.persist_index(&index)?;
        Ok(CacheStorePutOutcome { evicted })
    }

    fn remove(&self, key: &str) -> std::io::Result<()> {
        let mut index = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "cache lock poisoned"))?;
        index.remove_entry(key, &self.entries_path)?;
        self.persist_index(&index)?;
        Ok(())
    }

    fn clear(&self) -> std::io::Result<()> {
        let mut index = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "cache lock poisoned"))?;
        index.clear(&self.entries_path)?;
        self.persist_index(&index)?;
        Ok(())
    }

    fn stats(&self) -> std::io::Result<CacheStoreStats> {
        let index = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "cache lock poisoned"))?;
        Ok(CacheStoreStats {
            entries: index.entries.len(),
            total_bytes: index.total_bytes,
        })
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheIndex {
    entries: HashMap<String, CacheIndexEntry>,
    total_bytes: u64,
}

impl CacheIndex {
    fn remove_entry(&mut self, key: &str, entries_path: &Path) -> std::io::Result<()> {
        if let Some(entry) = self.entries.remove(key) {
            self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
            let entry_path = entries_path.join(key);
            let _ = std::fs::remove_file(entry_path);
        }
        Ok(())
    }

    fn clear(&mut self, entries_path: &Path) -> std::io::Result<()> {
        for key in self.entries.keys() {
            let _ = std::fs::remove_file(entries_path.join(key));
        }
        self.entries.clear();
        self.total_bytes = 0;
        Ok(())
    }

    fn oldest_entry(&self) -> Option<(String, &CacheIndexEntry)> {
        self.entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access_epoch)
            .map(|(key, entry)| (key.clone(), entry))
    }

    fn prune_expired(&mut self, entries_path: &Path) -> std::io::Result<()> {
        let now = now_epoch_secs();
        let expired_keys = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                if entry.is_expired_at(now) {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for key in expired_keys {
            self.remove_entry(&key, entries_path)?;
        }
        Ok(())
    }

    fn recalculate_bytes(&mut self, entries_path: &Path) -> std::io::Result<()> {
        let mut total = 0u64;
        let missing_keys = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                let path = entries_path.join(key);
                match std::fs::metadata(&path) {
                    Ok(metadata) => {
                        total = total.saturating_add(metadata.len());
                        None
                    }
                    Err(_) => Some((key.clone(), entry.size_bytes)),
                }
            })
            .collect::<Vec<_>>();
        for (key, size) in missing_keys {
            self.entries.remove(&key);
            self.total_bytes = self.total_bytes.saturating_sub(size);
        }
        self.total_bytes = total;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheIndexEntry {
    size_bytes: u64,
    inserted_epoch: u64,
    last_access_epoch: u64,
    ttl_secs: u64,
}

impl CacheIndexEntry {
    fn is_expired(&self) -> bool {
        self.is_expired_at(now_epoch_secs())
    }

    fn is_expired_at(&self, now: u64) -> bool {
        if self.ttl_secs == 0 {
            return true;
        }
        now.saturating_sub(self.inserted_epoch) > self.ttl_secs
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn stores_and_retrieves_values() -> std::io::Result<()> {
        let dir = tempdir()?;
        let store = DiskCacheStore::new(dir.path(), 1024)?;
        let entry = CacheEntry {
            key: "alpha".to_string(),
            value: b"one".to_vec(),
            ttl: Duration::from_secs(60),
        };

        store.put(entry)?;
        let cached = store.get("alpha")?.expect("cache entry");
        assert_eq!(cached.value, b"one".to_vec());
        Ok(())
    }

    #[test]
    fn evicts_when_over_capacity() -> std::io::Result<()> {
        let dir = tempdir()?;
        let store = DiskCacheStore::new(dir.path(), 10)?;
        store.put(CacheEntry {
            key: "alpha".to_string(),
            value: b"123456".to_vec(),
            ttl: Duration::from_secs(60),
        })?;
        store.put(CacheEntry {
            key: "bravo".to_string(),
            value: b"abcdef".to_vec(),
            ttl: Duration::from_secs(60),
        })?;

        assert!(store.get("alpha")?.is_none());
        assert!(store.get("bravo")?.is_some());
        Ok(())
    }

    #[test]
    fn expired_entries_are_not_returned() -> std::io::Result<()> {
        let dir = tempdir()?;
        let store = DiskCacheStore::new(dir.path(), 1024)?;
        store.put(CacheEntry {
            key: "alpha".to_string(),
            value: b"stale".to_vec(),
            ttl: Duration::from_secs(0),
        })?;

        assert!(store.get("alpha")?.is_none());
        Ok(())
    }

    #[test]
    fn clear_removes_entries() -> std::io::Result<()> {
        let dir = tempdir()?;
        let store = DiskCacheStore::new(dir.path(), 1024)?;
        store.put(CacheEntry {
            key: "alpha".to_string(),
            value: b"one".to_vec(),
            ttl: Duration::from_secs(60),
        })?;
        store.clear()?;

        assert!(store.get("alpha")?.is_none());
        Ok(())
    }
}
