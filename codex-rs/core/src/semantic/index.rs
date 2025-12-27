use crate::AuthManager;
use crate::model_provider_info::ModelProviderInfo;
use crate::semantic::LOG_TARGET;
use crate::semantic::config::SemanticIndexConfig;
use crate::semantic::embedding::EmbeddingClient;
use crate::semantic::vector_store::ChunkEntry;
use crate::semantic::vector_store::FileEntry;
use crate::semantic::vector_store::IndexMeta;
use crate::semantic::vector_store::IndexStats;
use crate::semantic::vector_store::StoreMode;
use crate::semantic::vector_store::VectorStore;
use anyhow::Context;
use anyhow::Result;
use chrono::Utc;
use sha2::Digest;
use sha2::Sha256;
use std::cmp::Ordering;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing::warn;
use walkdir::DirEntry;
use walkdir::WalkDir;

const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f32,
    pub chunk_id: String,
}

pub struct SemanticIndex {
    workspace_root: PathBuf,
    config: SemanticIndexConfig,
    provider: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
}

impl SemanticIndex {
    pub fn new(
        workspace_root: PathBuf,
        config: SemanticIndexConfig,
        provider: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            workspace_root,
            config,
            provider,
            auth_manager,
        }
    }

    pub async fn build(&self) -> Result<IndexStats> {
        if !self.config.enabled {
            anyhow::bail!("semantic index is disabled; enable it under [semantic_index]");
        }
        let index_dir = self.config.dir.as_path();
        let store = VectorStore::open(index_dir, StoreMode::Reset)?;
        let embedder =
            EmbeddingClient::new(self.provider.clone(), self.auth_manager.clone()).await?;
        let workspace_fingerprint = fingerprint_workspace(&self.workspace_root);
        let created_at = Utc::now();
        let mut embedding_dim: Option<usize> = None;

        info!(
            target: LOG_TARGET,
            index_dir = %index_dir.display(),
            "starting semantic index build",
        );

        let files = collect_files(&self.workspace_root, index_dir)?;
        for file_path in files {
            let relative = file_path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(&file_path);
            let relative_display = relative.to_string_lossy().to_string();
            let metadata = match fs::metadata(&file_path) {
                Ok(metadata) => metadata,
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        path = %file_path.display(),
                        "skipping file metadata error: {err}",
                    );
                    continue;
                }
            };
            let size = metadata.len();
            let modified = metadata
                .modified()
                .ok()
                .and_then(|ts| ts.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|ts| ts.as_secs() as i64)
                .unwrap_or(0);
            let bytes = match fs::read(&file_path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        path = %file_path.display(),
                        "skipping unreadable file: {err}",
                    );
                    continue;
                }
            };
            if bytes.is_empty() || bytes.contains(&0) {
                continue;
            }
            let contents = String::from_utf8_lossy(&bytes);
            let lines: Vec<String> = contents.lines().map(ToString::to_string).collect();
            let chunks = chunk_lines(&lines, self.config.chunk.max_lines);
            if chunks.is_empty() {
                continue;
            }

            let content_hash = hash_bytes(&bytes);
            store.store_file(&FileEntry {
                path: relative_display.clone(),
                content_hash,
                mtime: modified,
                size,
            })?;

            let chunk_texts: Vec<String> = chunks.iter().map(|chunk| chunk.text.clone()).collect();
            let embeddings = embedder
                .embed(&self.config.embedding_model, &chunk_texts)
                .await
                .with_context(|| format!("embedding failed for {}", file_path.display()))?;
            if embeddings.len() != chunks.len() {
                anyhow::bail!(
                    "embedding response mismatch for {} (expected {}, got {})",
                    file_path.display(),
                    chunks.len(),
                    embeddings.len()
                );
            }
            for (chunk, embedding) in chunks.into_iter().zip(embeddings) {
                if let Some(dim) = embedding_dim {
                    if dim != embedding.len() {
                        anyhow::bail!(
                            "embedding dimension changed from {dim} to {}",
                            embedding.len()
                        );
                    }
                } else {
                    embedding_dim = Some(embedding.len());
                }
                let text_hash = hash_string(&chunk.text);
                let chunk_id = chunk_id(
                    &relative_display,
                    chunk.start_line,
                    chunk.end_line,
                    &text_hash,
                );
                store.store_chunk(&ChunkEntry {
                    file_path: relative_display.clone(),
                    chunk_id,
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    text_hash,
                    embedding,
                    updated_at: created_at,
                })?;
            }
        }

        let meta = IndexMeta {
            schema_version: SCHEMA_VERSION,
            embedding_model: self.config.embedding_model.clone(),
            dim: embedding_dim.unwrap_or(0),
            chunk_size: self.config.chunk.max_lines,
            created_at,
            workspace_fingerprint,
        };
        store.store_meta(&meta)?;
        let stats = store.stats()?;
        info!(
            target: LOG_TARGET,
            files = stats.file_count,
            chunks = stats.chunk_count,
            "semantic index build complete",
        );
        Ok(stats)
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let store = VectorStore::open(self.config.dir.as_path(), StoreMode::OpenExisting)?;
        store.stats()
    }

    pub fn clear(&self) -> Result<()> {
        VectorStore::clear(self.config.dir.as_path())
    }

    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
        if !self.config.enabled {
            anyhow::bail!("semantic index is disabled; enable it under [semantic_index]");
        }
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let store = VectorStore::open(self.config.dir.as_path(), StoreMode::OpenExisting)?;
        let embedder =
            EmbeddingClient::new(self.provider.clone(), self.auth_manager.clone()).await?;
        let embedding = embedder
            .embed(&self.config.embedding_model, &[query.to_string()])
            .await?
            .into_iter()
            .next()
            .context("missing embedding result")?;
        let candidates = store.list_embeddings()?;
        let mut scored: Vec<SearchHit> = candidates
            .into_iter()
            .filter_map(|candidate| {
                let score = cosine_similarity(&embedding, &candidate.embedding)?;
                Some(SearchHit {
                    file_path: candidate.file_path,
                    start_line: candidate.start_line,
                    end_line: candidate.end_line,
                    score,
                    chunk_id: candidate.chunk_id,
                })
            })
            .collect();
        scored.sort_by(score_cmp);
        scored.truncate(top_k);
        Ok(scored)
    }
}

fn collect_files(workspace_root: &Path, index_dir: &Path) -> Result<Vec<PathBuf>> {
    let walker = WalkDir::new(workspace_root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(entry, workspace_root, index_dir));
    let mut files = Vec::new();
    for entry in walker {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

fn should_skip_entry(entry: &DirEntry, workspace_root: &Path, index_dir: &Path) -> bool {
    let path = entry.path();
    if path == index_dir {
        return true;
    }
    if path.starts_with(index_dir) {
        return true;
    }
    if let Ok(relative) = path.strip_prefix(workspace_root)
        && relative.components().any(|comp| comp.as_os_str() == ".git")
    {
        return true;
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Chunk {
    start_line: usize,
    end_line: usize,
    text: String,
}

fn chunk_lines(lines: &[String], max_lines: usize) -> Vec<Chunk> {
    if max_lines == 0 {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    for (chunk_index, chunk_lines) in lines.chunks(max_lines).enumerate() {
        let start_line = chunk_index * max_lines + 1;
        let end_line = start_line + chunk_lines.len().saturating_sub(1);
        let text = chunk_lines.join("\n");
        if text.trim().is_empty() {
            continue;
        }
        chunks.push(Chunk {
            start_line,
            end_line,
            text,
        });
    }
    chunks
}

fn chunk_id(path: &str, start_line: usize, end_line: usize, text_hash: &str) -> String {
    let input = format!("{path}:{start_line}-{end_line}:{text_hash}");
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hash_string(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn fingerprint_workspace(path: &Path) -> String {
    hash_string(path.to_string_lossy().as_ref())
}

fn cosine_similarity(query: &[f32], other: &[f32]) -> Option<f32> {
    if query.len() != other.len() || query.is_empty() {
        return None;
    }
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (a, b) in query.iter().zip(other) {
        dot += a * b;
        norm_a += a * a;
        norm_b += b * b;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        None
    } else {
        Some(dot / denom)
    }
}

fn score_cmp(a: &SearchHit, b: &SearchHit) -> Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.file_path.cmp(&b.file_path))
        .then_with(|| a.start_line.cmp(&b.start_line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn chunk_lines_splits_by_max_lines() {
        let lines = vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
        ];
        let chunks = chunk_lines(&lines, 2);
        let expected = vec![
            Chunk {
                start_line: 1,
                end_line: 2,
                text: "one\ntwo".to_string(),
            },
            Chunk {
                start_line: 3,
                end_line: 4,
                text: "three\nfour".to_string(),
            },
        ];
        assert_eq!(chunks, expected);
    }

    #[test]
    fn cosine_similarity_returns_none_for_mismatch() {
        let a = vec![1.0_f32, 2.0_f32];
        let b = vec![1.0_f32];
        assert_eq!(cosine_similarity(&a, &b), None);
    }
}
