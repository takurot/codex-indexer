use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub key: String,
    pub value: Vec<u8>,
    pub ttl: Duration,
}

pub trait CacheStore: Send + Sync {
    fn get(&self, key: &str) -> std::io::Result<Option<CacheEntry>>;
    fn put(&self, entry: CacheEntry) -> std::io::Result<()>;
    fn remove(&self, key: &str) -> std::io::Result<()>;
    fn clear(&self) -> std::io::Result<()>;
}
