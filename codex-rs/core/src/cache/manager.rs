use crate::cache::LOG_TARGET;
use crate::cache::config::CacheConfig;
use crate::cache::config::CacheableTool;
use crate::cache::store::CacheEntry;
use crate::cache::store::CacheStore;
use crate::cache::store::CacheStorePutOutcome;
use crate::cache::store::CacheStoreStats;
use crate::cache::store::DiskCacheStore;
use crate::telemetry::CacheTelemetry;
use crate::telemetry::CacheTelemetrySnapshot;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

#[derive(Debug, Clone, PartialEq)]
pub struct CacheStatus {
    pub enabled: bool,
    pub dir: AbsolutePathBuf,
    pub max_bytes: u64,
    pub stats: CacheStoreStats,
    pub telemetry: CacheTelemetrySnapshot,
}

pub struct CacheManager {
    config: CacheConfig,
    store: Arc<dyn CacheStore>,
    telemetry: CacheTelemetry,
}

impl CacheManager {
    pub fn new(config: CacheConfig) -> std::io::Result<Self> {
        let store = DiskCacheStore::new(config.dir.as_path(), config.max_bytes)?;
        Ok(Self {
            config,
            store: Arc::new(store),
            telemetry: CacheTelemetry::default(),
        })
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn ttl_for(&self, tool: CacheableTool) -> Duration {
        self.config.ttl_for(tool)
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        if !self.enabled() {
            return None;
        }
        match self.store.get(key) {
            Ok(Some(entry)) => {
                self.telemetry.record_hit();
                Some(entry.value)
            }
            Ok(None) => {
                self.telemetry.record_miss();
                None
            }
            Err(err) => {
                warn!(target: LOG_TARGET, "cache lookup failed: {err}");
                None
            }
        }
    }

    pub fn put(&self, key: String, value: Vec<u8>, ttl: Duration) {
        if !self.enabled() {
            return;
        }
        let entry = CacheEntry { key, value, ttl };
        match self.store.put(entry) {
            Ok(CacheStorePutOutcome { evicted }) => {
                self.telemetry.record_store();
                for _ in 0..evicted {
                    self.telemetry.record_eviction();
                }
            }
            Err(err) => {
                warn!(target: LOG_TARGET, "cache store failed: {err}");
            }
        }
    }

    pub fn clear(&self) -> std::io::Result<()> {
        self.store.clear()
    }

    pub fn status(&self) -> std::io::Result<CacheStatus> {
        let stats = self.store.stats()?;
        Ok(CacheStatus {
            enabled: self.enabled(),
            dir: self.config.dir.clone(),
            max_bytes: self.config.max_bytes,
            stats,
            telemetry: self.telemetry.snapshot(),
        })
    }
}
