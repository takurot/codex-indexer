use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use rusqlite::Connection;
use rusqlite::params;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use thiserror::Error;

const DB_FILE_NAME: &str = "index.sqlite";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMeta {
    pub schema_version: i32,
    pub embedding_model: String,
    pub dim: usize,
    pub chunk_size: usize,
    pub created_at: DateTime<Utc>,
    pub workspace_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStats {
    pub file_count: usize,
    pub chunk_count: usize,
    pub embedding_model: Option<String>,
    pub embedding_dim: Option<usize>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub content_hash: String,
    pub mtime: i64,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkEntry {
    pub file_path: String,
    pub chunk_id: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text_hash: String,
    pub embedding: Vec<f32>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRecord {
    pub file_path: String,
    pub chunk_id: String,
    pub start_line: usize,
    pub end_line: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    OpenExisting,
    CreateOrOpen,
    Reset,
}

pub struct VectorStore {
    conn: Connection,
    db_path: PathBuf,
}

impl VectorStore {
    pub fn open(dir: &Path, mode: StoreMode) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| {
            format!(
                "failed to create semantic index directory {}",
                dir.display()
            )
        })?;
        let db_path = dir.join(DB_FILE_NAME);
        match mode {
            StoreMode::Reset => {
                if db_path.exists() {
                    fs::remove_file(&db_path).with_context(|| {
                        format!("failed to remove semantic index {}", db_path.display())
                    })?;
                }
            }
            StoreMode::OpenExisting => {
                if !db_path.exists() {
                    anyhow::bail!("semantic index not found at {}", db_path.display());
                }
            }
            StoreMode::CreateOrOpen => {}
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open semantic index {}", db_path.display()))?;
        let store = Self { conn, db_path };
        store.init_schema()?;
        Ok(store)
    }

    pub fn clear(dir: &Path) -> Result<()> {
        let db_path = dir.join(DB_FILE_NAME);
        if db_path.exists() {
            fs::remove_file(&db_path).with_context(|| {
                format!("failed to remove semantic index {}", db_path.display())
            })?;
        }
        Ok(())
    }

    pub fn store_meta(&self, meta: &IndexMeta) -> Result<()> {
        let created_at = meta.created_at.to_rfc3339();
        self.conn.execute("DELETE FROM meta", [])?;
        self.conn.execute(
            "INSERT INTO meta (id, schema_version, embedding_model, dim, chunk_size, created_at, workspace_fingerprint)
             VALUES (1, ?, ?, ?, ?, ?, ?)",
            params![
                meta.schema_version,
                meta.embedding_model,
                meta.dim as i64,
                meta.chunk_size as i64,
                created_at,
                meta.workspace_fingerprint
            ],
        )?;
        Ok(())
    }

    pub fn store_file(&self, file: &FileEntry) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO files (path, content_hash, mtime, size) VALUES (?, ?, ?, ?)",
            params![file.path, file.content_hash, file.mtime, file.size as i64],
        )?;
        Ok(())
    }

    pub fn store_chunk(&self, chunk: &ChunkEntry) -> Result<()> {
        let updated_at = chunk.updated_at.to_rfc3339();
        let embedding = encode_embedding(&chunk.embedding);
        self.conn.execute(
            "INSERT OR REPLACE INTO chunks (file_path, chunk_id, start_line, end_line, text_hash, embedding, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                chunk.file_path,
                chunk.chunk_id,
                chunk.start_line as i64,
                chunk.end_line as i64,
                chunk.text_hash,
                embedding,
                updated_at
            ],
        )?;
        Ok(())
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let file_count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| {
                Ok(row.get::<_, i64>(0)? as usize)
            })?;
        let chunk_count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                Ok(row.get::<_, i64>(0)? as usize)
            })?;
        let mut stmt = self
            .conn
            .prepare("SELECT embedding_model, dim, created_at FROM meta WHERE id = 1 LIMIT 1")?;
        let mut rows = stmt.query([])?;
        let meta_row = rows.next()?;
        let (embedding_model, embedding_dim, created_at) = if let Some(row) = meta_row {
            let model: String = row.get(0)?;
            let dim = row.get::<_, i64>(1)? as usize;
            let created_at: String = row.get(2)?;
            let parsed = DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .ok();
            (Some(model), Some(dim), parsed)
        } else {
            (None, None, None)
        };
        Ok(IndexStats {
            file_count,
            chunk_count,
            embedding_model,
            embedding_dim,
            created_at,
        })
    }

    pub fn list_embeddings(&self) -> Result<Vec<EmbeddingRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, chunk_id, start_line, end_line, embedding FROM chunks")?;
        let rows = stmt.query_map([], |row| {
            let embedding: Vec<u8> = row.get(4)?;
            let embedding = decode_embedding(&embedding).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    embedding.len(),
                    rusqlite::types::Type::Blob,
                    Box::new(err),
                )
            })?;
            Ok(EmbeddingRecord {
                file_path: row.get(0)?,
                chunk_id: row.get(1)?,
                start_line: row.get::<_, i64>(2)? as usize,
                end_line: row.get::<_, i64>(3)? as usize,
                embedding,
            })
        })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                schema_version INTEGER NOT NULL,
                embedding_model TEXT NOT NULL,
                dim INTEGER NOT NULL,
                chunk_size INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                workspace_fingerprint TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chunks (
                file_path TEXT NOT NULL,
                chunk_id TEXT PRIMARY KEY,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                text_hash TEXT NOT NULL,
                embedding BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS chunks_by_file ON chunks(file_path);",
        )?;
        Ok(())
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(std::mem::size_of_val(embedding));
    for value in embedding {
        buf.extend_from_slice(&value.to_le_bytes());
    }
    buf
}

#[derive(Debug, Error)]
#[error("embedding blob length {len} is not a multiple of {element_size}")]
struct EmbeddingDecodeError {
    len: usize,
    element_size: usize,
}

fn decode_embedding(bytes: &[u8]) -> std::result::Result<Vec<f32>, EmbeddingDecodeError> {
    let size = std::mem::size_of::<f32>();
    if !bytes.len().is_multiple_of(size) {
        return Err(EmbeddingDecodeError {
            len: bytes.len(),
            element_size: size,
        });
    }
    let mut values = Vec::with_capacity(bytes.len() / size);
    for chunk in bytes.chunks_exact(size) {
        let mut array = [0u8; 4];
        array.copy_from_slice(chunk);
        values.push(f32::from_le_bytes(array));
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn encode_decode_round_trip() {
        let values = vec![0.25_f32, -1.0_f32, 4.5_f32];
        let encoded = encode_embedding(&values);
        let decoded = decode_embedding(&encoded).expect("decode");
        assert_eq!(decoded, values);
    }

    #[test]
    fn stats_empty_when_missing_meta() {
        let dir = tempdir().expect("tempdir");
        let store = VectorStore::open(dir.path(), StoreMode::CreateOrOpen).expect("open");
        let stats = store.stats().expect("stats");
        let expected = IndexStats {
            file_count: 0,
            chunk_count: 0,
            embedding_model: None,
            embedding_dim: None,
            created_at: None,
        };
        assert_eq!(stats, expected);
    }
}
