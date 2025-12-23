use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tracing::trace;

use crate::cache::LOG_TARGET;
use crate::cache::config::CacheableTool;

/// Lightweight metrics collector for cache operations.
#[derive(Debug)]
pub struct CacheTelemetry {
    overall: CacheCounters,
    by_tool: [CacheCounters; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheTelemetrySnapshot {
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
    pub hit_rate: Option<f64>,
    pub by_tool: Vec<CacheToolTelemetrySnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheToolTelemetrySnapshot {
    pub tool: CacheableTool,
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
    pub hit_rate: Option<f64>,
}

#[derive(Debug, Default)]
struct CacheCounters {
    hits: AtomicU64,
    misses: AtomicU64,
    stores: AtomicU64,
    evictions: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
struct CacheCountersSnapshot {
    hits: u64,
    misses: u64,
    stores: u64,
    evictions: u64,
}

impl CacheTelemetry {
    pub fn record_hit(&self, tool: CacheableTool) {
        self.overall.record_hit();
        self.by_tool[tool_index(tool)].record_hit();
    }

    pub fn record_miss(&self, tool: CacheableTool) {
        self.overall.record_miss();
        self.by_tool[tool_index(tool)].record_miss();
    }

    pub fn record_store(&self, tool: CacheableTool) {
        self.overall.record_store();
        self.by_tool[tool_index(tool)].record_store();
    }

    pub fn record_eviction(&self, tool: CacheableTool) {
        self.overall.record_eviction();
        self.by_tool[tool_index(tool)].record_eviction();
    }

    pub fn snapshot(&self) -> CacheTelemetrySnapshot {
        let overall = self.overall.snapshot();
        let mut by_tool = Vec::with_capacity(CacheableTool::all().len());
        for tool in CacheableTool::all() {
            let snapshot = self.by_tool[tool_index(*tool)].snapshot();
            by_tool.push(CacheToolTelemetrySnapshot {
                tool: *tool,
                hits: snapshot.hits,
                misses: snapshot.misses,
                stores: snapshot.stores,
                evictions: snapshot.evictions,
                hit_rate: hit_rate(snapshot.hits, snapshot.misses),
            });
        }

        CacheTelemetrySnapshot {
            hits: overall.hits,
            misses: overall.misses,
            stores: overall.stores,
            evictions: overall.evictions,
            hit_rate: hit_rate(overall.hits, overall.misses),
            by_tool,
        }
    }
}

impl Default for CacheTelemetry {
    fn default() -> Self {
        Self {
            overall: CacheCounters::default(),
            by_tool: [
                CacheCounters::default(),
                CacheCounters::default(),
                CacheCounters::default(),
            ],
        }
    }
}

impl CacheCounters {
    fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache hit recorded");
    }

    fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache miss recorded");
    }

    fn record_store(&self) {
        self.stores.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache store recorded");
    }

    fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
        trace!(target: LOG_TARGET, "cache eviction recorded");
    }

    fn snapshot(&self) -> CacheCountersSnapshot {
        CacheCountersSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            stores: self.stores.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }
}

fn hit_rate(hits: u64, misses: u64) -> Option<f64> {
    let lookups = hits + misses;
    if lookups == 0 {
        None
    } else {
        Some(hits as f64 / lookups as f64)
    }
}

fn tool_index(tool: CacheableTool) -> usize {
    match tool {
        CacheableTool::ReadFile => 0,
        CacheableTool::ListDir => 1,
        CacheableTool::GrepFiles => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn captures_hit_rate_and_counts() {
        let telemetry = CacheTelemetry::default();

        telemetry.record_hit(CacheableTool::ReadFile);
        telemetry.record_hit(CacheableTool::GrepFiles);
        telemetry.record_miss(CacheableTool::ListDir);
        telemetry.record_store(CacheableTool::ReadFile);
        telemetry.record_eviction(CacheableTool::ListDir);

        let snapshot = telemetry.snapshot();

        assert_eq!(snapshot.hits, 2);
        assert_eq!(snapshot.misses, 1);
        assert_eq!(snapshot.stores, 1);
        assert_eq!(snapshot.evictions, 1);
        assert_eq!(snapshot.hit_rate, Some(2.0 / 3.0));
        assert_eq!(snapshot.by_tool.len(), 3);
        assert_eq!(
            snapshot.by_tool[0],
            CacheToolTelemetrySnapshot {
                tool: CacheableTool::ReadFile,
                hits: 1,
                misses: 0,
                stores: 1,
                evictions: 0,
                hit_rate: Some(1.0)
            }
        );
        assert_eq!(
            snapshot.by_tool[1],
            CacheToolTelemetrySnapshot {
                tool: CacheableTool::ListDir,
                hits: 0,
                misses: 1,
                stores: 0,
                evictions: 1,
                hit_rate: Some(0.0)
            }
        );
        assert_eq!(
            snapshot.by_tool[2],
            CacheToolTelemetrySnapshot {
                tool: CacheableTool::GrepFiles,
                hits: 1,
                misses: 0,
                stores: 0,
                evictions: 0,
                hit_rate: Some(1.0)
            }
        );
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
                by_tool: vec![
                    CacheToolTelemetrySnapshot {
                        tool: CacheableTool::ReadFile,
                        hits: 0,
                        misses: 0,
                        stores: 0,
                        evictions: 0,
                        hit_rate: None
                    },
                    CacheToolTelemetrySnapshot {
                        tool: CacheableTool::ListDir,
                        hits: 0,
                        misses: 0,
                        stores: 0,
                        evictions: 0,
                        hit_rate: None
                    },
                    CacheToolTelemetrySnapshot {
                        tool: CacheableTool::GrepFiles,
                        hits: 0,
                        misses: 0,
                        stores: 0,
                        evictions: 0,
                        hit_rate: None
                    },
                ],
            }
        );
    }
}
