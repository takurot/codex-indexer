use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tracing::trace;

use crate::cache::LOG_TARGET;

/// Lightweight metrics collector for cache operations.
#[derive(Debug, Default)]
pub struct CacheTelemetry {
    hits: AtomicU64,
    misses: AtomicU64,
    stores: AtomicU64,
    evictions: AtomicU64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheTelemetrySnapshot {
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
    pub hit_rate: Option<f64>,
}

impl CacheTelemetry {
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache hit recorded");
    }

    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache miss recorded");
    }

    pub fn record_store(&self) {
        self.stores.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache store recorded");
    }

    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache eviction recorded");
    }

    pub fn snapshot(&self) -> CacheTelemetrySnapshot {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let stores = self.stores.load(Ordering::Relaxed);
        let evictions = self.evictions.load(Ordering::Relaxed);
        let lookups = hits + misses;
        let hit_rate = if lookups == 0 {
            None
        } else {
            Some(hits as f64 / lookups as f64)
        };

        CacheTelemetrySnapshot {
            hits,
            misses,
            stores,
            evictions,
            hit_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn captures_hit_rate_and_counts() {
        let telemetry = CacheTelemetry::default();

        telemetry.record_hit();
        telemetry.record_hit();
        telemetry.record_miss();
        telemetry.record_store();
        telemetry.record_eviction();

        let snapshot = telemetry.snapshot();

        assert_eq!(snapshot.hits, 2);
        assert_eq!(snapshot.misses, 1);
        assert_eq!(snapshot.stores, 1);
        assert_eq!(snapshot.evictions, 1);
        assert_eq!(snapshot.hit_rate, Some(2.0 / 3.0));
    }

    #[test]
    fn hit_rate_is_none_without_samples() {
        let telemetry = CacheTelemetry::default();

        let snapshot = telemetry.snapshot();

        assert_eq!(
            snapshot,
            CacheTelemetrySnapshot {
                hits: 0,
                misses: 0,
                stores: 0,
                evictions: 0,
                hit_rate: None,
            }
        );
    }
}
