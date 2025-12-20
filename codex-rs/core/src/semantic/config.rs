use crate::semantic::LOG_TARGET;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use std::path::Path;
use tracing::debug;

pub const DEFAULT_SEMANTIC_INDEX_DIR: &str = ".codex-index";
pub const DEFAULT_SEMANTIC_INDEX_MODEL: &str = "text-embedding-3-small";
pub const DEFAULT_SEMANTIC_INDEX_CHUNK_MAX_LINES: usize = 120;
pub const DEFAULT_SEMANTIC_INDEX_RETRIEVE_TOP_K: usize = 8;
pub const DEFAULT_SEMANTIC_INDEX_RETRIEVE_MAX_CHARS: usize = 12_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIndexConfig {
    pub enabled: bool,
    pub dir: AbsolutePathBuf,
    pub embedding_model: String,
    pub chunk: ChunkingConfig,
    pub retrieve: RetrieveConfig,
}

impl SemanticIndexConfig {
    pub fn new(
        workspace_root: &Path,
        semantic: Option<SemanticIndexConfigToml>,
    ) -> std::io::Result<Self> {
        let semantic = semantic.unwrap_or_default();
        let dir = match semantic.dir {
            Some(dir) => AbsolutePathBuf::resolve_path_against_base(dir, workspace_root)?,
            None => AbsolutePathBuf::resolve_path_against_base(
                DEFAULT_SEMANTIC_INDEX_DIR,
                workspace_root,
            )?,
        };
        let chunk = ChunkingConfig {
            max_lines: semantic
                .chunk
                .max_lines
                .unwrap_or(DEFAULT_SEMANTIC_INDEX_CHUNK_MAX_LINES),
        };
        let retrieve = RetrieveConfig {
            top_k: semantic
                .retrieve
                .top_k
                .unwrap_or(DEFAULT_SEMANTIC_INDEX_RETRIEVE_TOP_K),
            max_chars: semantic
                .retrieve
                .max_chars
                .unwrap_or(DEFAULT_SEMANTIC_INDEX_RETRIEVE_MAX_CHARS),
        };

        debug!(
            target: LOG_TARGET,
            enabled = semantic.enabled.unwrap_or(true),
            dir = %dir.display(),
            embedding_model = semantic
                .embedding_model
                .as_deref()
                .unwrap_or(DEFAULT_SEMANTIC_INDEX_MODEL),
            chunk_max_lines = chunk.max_lines,
            retrieve_top_k = retrieve.top_k,
            retrieve_max_chars = retrieve.max_chars,
            "loaded semantic index config",
        );

        Ok(Self {
            enabled: semantic.enabled.unwrap_or(true),
            dir,
            embedding_model: semantic
                .embedding_model
                .unwrap_or_else(|| DEFAULT_SEMANTIC_INDEX_MODEL.to_string()),
            chunk,
            retrieve,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkingConfig {
    pub max_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrieveConfig {
    pub top_k: usize,
    pub max_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct SemanticIndexConfigToml {
    pub enabled: Option<bool>,
    pub dir: Option<std::path::PathBuf>,
    pub embedding_model: Option<String>,
    #[serde(default)]
    pub chunk: ChunkingConfigToml,
    #[serde(default)]
    pub retrieve: RetrieveConfigToml,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct ChunkingConfigToml {
    pub max_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct RetrieveConfigToml {
    pub top_k: Option<usize>,
    pub max_chars: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn defaults_resolve_workspace_relative_dir() {
        let workspace = tempdir().expect("tempdir");
        let config =
            SemanticIndexConfig::new(workspace.path(), None).expect("semantic index config");

        let expected_dir = AbsolutePathBuf::resolve_path_against_base(
            DEFAULT_SEMANTIC_INDEX_DIR,
            workspace.path(),
        )
        .expect("resolve default dir");

        assert!(config.enabled);
        assert_eq!(config.dir, expected_dir);
        assert_eq!(config.embedding_model, DEFAULT_SEMANTIC_INDEX_MODEL);
        assert_eq!(
            config.chunk.max_lines,
            DEFAULT_SEMANTIC_INDEX_CHUNK_MAX_LINES
        );
        assert_eq!(config.retrieve.top_k, DEFAULT_SEMANTIC_INDEX_RETRIEVE_TOP_K);
        assert_eq!(
            config.retrieve.max_chars,
            DEFAULT_SEMANTIC_INDEX_RETRIEVE_MAX_CHARS
        );
    }

    #[test]
    fn overrides_are_resolved_against_workspace() {
        let workspace = tempdir().expect("tempdir");
        let semantic = SemanticIndexConfigToml {
            enabled: Some(false),
            dir: Some(std::path::PathBuf::from("custom-index")),
            embedding_model: Some("model-x".to_string()),
            chunk: ChunkingConfigToml {
                max_lines: Some(42),
            },
            retrieve: RetrieveConfigToml {
                top_k: Some(5),
                max_chars: Some(1024),
            },
        };

        let config =
            SemanticIndexConfig::new(workspace.path(), Some(semantic)).expect("semantic index");

        let expected_dir =
            AbsolutePathBuf::resolve_path_against_base("custom-index", workspace.path())
                .expect("resolve dir");
        assert!(!config.enabled);
        assert_eq!(config.dir, expected_dir);
        assert_eq!(config.embedding_model, "model-x");
        assert_eq!(config.chunk.max_lines, 42);
        assert_eq!(config.retrieve.top_k, 5);
        assert_eq!(config.retrieve.max_chars, 1024);
    }
}
