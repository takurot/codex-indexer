use crate::cache::LOG_TARGET;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;
use tracing::debug;

pub const DEFAULT_CACHE_DIR_NAME: &str = "cache";
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_CACHE_DEFAULT_TTL_SECS: u64 = 60;
pub const DEFAULT_CACHE_READ_FILE_TTL_SECS: u64 = 300;
pub const DEFAULT_CACHE_GREP_FILES_TTL_SECS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheableTool {
    ReadFile,
    ListDir,
    GrepFiles,
}

impl CacheableTool {
    pub fn config_key(self) -> &'static str {
        match self {
            CacheableTool::ReadFile => "read_file",
            CacheableTool::ListDir => "list_dir",
            CacheableTool::GrepFiles => "grep_files",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    pub enabled: bool,
    pub dir: AbsolutePathBuf,
    pub max_bytes: u64,
    pub default_ttl: Duration,
    pub tool_ttl: CacheToolTtl,
}

impl CacheConfig {
    pub fn new(codex_home: &Path, cache: Option<CacheConfigToml>) -> Self {
        let cache = cache.unwrap_or_default();
        let default_ttl = Duration::from_secs(
            cache
                .default_ttl_sec
                .unwrap_or(DEFAULT_CACHE_DEFAULT_TTL_SECS),
        );
        let dir = cache.dir.unwrap_or_else(|| {
            AbsolutePathBuf::resolve_path_against_base(DEFAULT_CACHE_DIR_NAME, codex_home)
                .expect("default cache dir should resolve")
        });
        let mut tool_ttl = CacheToolTtl::default();
        tool_ttl.override_with(&cache.tool_ttl_sec);

        debug!(
            target: LOG_TARGET,
            enabled = cache.enabled.unwrap_or(true),
            dir = %dir.display(),
            max_bytes = cache.max_bytes.unwrap_or(DEFAULT_CACHE_MAX_BYTES),
            default_ttl_secs = default_ttl.as_secs(),
            "loaded cache config",
        );

        Self {
            enabled: cache.enabled.unwrap_or(true),
            dir,
            max_bytes: cache.max_bytes.unwrap_or(DEFAULT_CACHE_MAX_BYTES),
            default_ttl,
            tool_ttl,
        }
    }

    pub fn ttl_for(&self, tool: CacheableTool) -> Duration {
        self.tool_ttl.for_tool(tool).unwrap_or(self.default_ttl)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheToolTtl {
    pub read_file: Option<Duration>,
    pub list_dir: Option<Duration>,
    pub grep_files: Option<Duration>,
}

impl CacheToolTtl {
    pub fn for_tool(&self, tool: CacheableTool) -> Option<Duration> {
        match tool {
            CacheableTool::ReadFile => self.read_file,
            CacheableTool::ListDir => self.list_dir,
            CacheableTool::GrepFiles => self.grep_files,
        }
    }

    fn override_with(&mut self, overrides: &CacheToolTtlToml) {
        if let Some(ttl) = overrides.read_file {
            self.read_file = Some(Duration::from_secs(ttl));
        }
        if let Some(ttl) = overrides.list_dir {
            self.list_dir = Some(Duration::from_secs(ttl));
        }
        if let Some(ttl) = overrides.grep_files {
            self.grep_files = Some(Duration::from_secs(ttl));
        }
    }
}

impl Default for CacheToolTtl {
    fn default() -> Self {
        Self {
            read_file: Some(Duration::from_secs(DEFAULT_CACHE_READ_FILE_TTL_SECS)),
            list_dir: None,
            grep_files: Some(Duration::from_secs(DEFAULT_CACHE_GREP_FILES_TTL_SECS)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct CacheConfigToml {
    pub enabled: Option<bool>,
    pub dir: Option<AbsolutePathBuf>,
    pub max_bytes: Option<u64>,
    pub default_ttl_sec: Option<u64>,
    #[serde(default)]
    pub tool_ttl_sec: CacheToolTtlToml,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct CacheToolTtlToml {
    pub read_file: Option<u64>,
    pub list_dir: Option<u64>,
    pub grep_files: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn defaults_use_codex_home_and_tool_overrides() {
        let codex_home = tempdir().expect("tempdir");

        let config = CacheConfig::new(codex_home.path(), None);

        let expected_dir =
            AbsolutePathBuf::resolve_path_against_base(DEFAULT_CACHE_DIR_NAME, codex_home.path())
                .expect("resolve default cache dir");
        assert!(config.enabled);
        assert_eq!(config.dir, expected_dir);
        assert_eq!(config.max_bytes, DEFAULT_CACHE_MAX_BYTES);
        assert_eq!(
            config.default_ttl,
            Duration::from_secs(DEFAULT_CACHE_DEFAULT_TTL_SECS)
        );
        assert_eq!(
            config.ttl_for(CacheableTool::ReadFile),
            Duration::from_secs(DEFAULT_CACHE_READ_FILE_TTL_SECS)
        );
        assert_eq!(
            config.ttl_for(CacheableTool::GrepFiles),
            Duration::from_secs(DEFAULT_CACHE_GREP_FILES_TTL_SECS)
        );
        assert_eq!(
            config.ttl_for(CacheableTool::ListDir),
            Duration::from_secs(DEFAULT_CACHE_DEFAULT_TTL_SECS)
        );
    }

    #[test]
    fn honors_overrides_and_ttl_lookup() {
        let codex_home = tempdir().expect("tempdir");
        let cache_dir =
            AbsolutePathBuf::resolve_path_against_base("cache_override", codex_home.path())
                .expect("resolve dir");
        let cache = CacheConfigToml {
            enabled: Some(false),
            dir: Some(cache_dir.clone()),
            max_bytes: Some(1024),
            default_ttl_sec: Some(5),
            tool_ttl_sec: CacheToolTtlToml {
                read_file: Some(1),
                list_dir: Some(2),
                grep_files: Some(3),
            },
        };

        let config = CacheConfig::new(codex_home.path(), Some(cache));

        assert!(!config.enabled);
        assert_eq!(config.dir, cache_dir);
        assert_eq!(config.max_bytes, 1024);
        assert_eq!(config.default_ttl, Duration::from_secs(5));
        assert_eq!(
            config.ttl_for(CacheableTool::ReadFile),
            Duration::from_secs(1)
        );
        assert_eq!(
            config.ttl_for(CacheableTool::ListDir),
            Duration::from_secs(2)
        );
        assert_eq!(
            config.ttl_for(CacheableTool::GrepFiles),
            Duration::from_secs(3)
        );
    }
}
