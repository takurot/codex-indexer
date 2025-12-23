use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use sha2::Digest;
use sha2::Sha256;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::warn;

use crate::cache::LOG_TARGET;
use crate::cache::config::CacheableTool;
use crate::cache::config::DEFAULT_CACHE_GREP_FILES_TTL_SECS;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct GrepFilesHandler;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Deserialize)]
struct GrepFilesArgs {
    pattern: String,
    #[serde(default)]
    include: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoState {
    head_ref: Option<String>,
    index_mtime_nanos: Option<u128>,
}

struct GrepCacheKeyInputs<'a> {
    workspace_root: &'a Path,
    search_path: &'a Path,
    pattern: &'a str,
    include: Option<&'a str>,
    limit: usize,
    repo_state: Option<&'a RepoState>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedGrepOutput {
    content: String,
    success: Option<bool>,
}

fn build_grep_cache_key(inputs: &GrepCacheKeyInputs<'_>) -> std::io::Result<String> {
    let GrepCacheKeyInputs {
        workspace_root,
        search_path,
        pattern,
        include,
        limit,
        repo_state,
    } = inputs;
    let fingerprint = serde_json::json!({
        "tool": "grep_files",
        "workspace": normalize_path(workspace_root),
        "path": normalize_path(search_path),
        "pattern": pattern,
        "include": include,
        "limit": limit,
        "git": repo_state.map(|state| serde_json::json!({
            "head": state.head_ref,
            "index_mtime": state.index_mtime_nanos,
        })),
    });
    let canonical = canonical_json(&fingerprint);
    let serialized = serde_json::to_string(&canonical).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to serialize cache key: {err}"),
        )
    })?;

    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let hash = hasher.finalize();
    let hex = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(hex)
}

fn cache_ttl_for_repo_state(configured: Duration, repo_state: Option<&RepoState>) -> Duration {
    if repo_state.is_some() {
        return configured;
    }
    configured.min(Duration::from_secs(DEFAULT_CACHE_GREP_FILES_TTL_SECS))
}

async fn detect_repo_state(workspace_root: &Path) -> Option<RepoState> {
    let git_dir = resolve_git_dir(workspace_root).await?;
    let head_ref = fs::read_to_string(git_dir.join("HEAD"))
        .await
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let index_mtime_nanos = fs::metadata(git_dir.join("index"))
        .await
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|mtime| mtime.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());

    if head_ref.is_none() && index_mtime_nanos.is_none() {
        return None;
    }

    Some(RepoState {
        head_ref,
        index_mtime_nanos,
    })
}

async fn resolve_git_dir(workspace_root: &Path) -> Option<PathBuf> {
    let mut cursor = workspace_root.to_path_buf();
    loop {
        let candidate = cursor.join(".git");
        match fs::metadata(&candidate).await {
            Ok(metadata) => {
                if metadata.is_dir() {
                    return Some(candidate);
                }
                if metadata.is_file() {
                    return parse_gitdir_file(&candidate, &cursor).await;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }

        if let Some(parent) = cursor.parent() {
            cursor = parent.to_path_buf();
        } else {
            return None;
        }
    }
}

async fn parse_gitdir_file(path: &Path, repo_root: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(path).await.ok()?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("gitdir:") {
            let gitdir = rest.trim();
            if gitdir.is_empty() {
                return None;
            }
            let candidate = PathBuf::from(gitdir);
            return if candidate.is_absolute() {
                Some(candidate)
            } else {
                Some(repo_root.join(candidate))
            };
        }
    }
    None
}

fn canonical_json(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                if let Some(val) = map.get(&key) {
                    sorted.insert(key, canonical_json(val));
                }
            }
            JsonValue::Object(sorted)
        }
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn decode_cached_output(bytes: &[u8]) -> Option<CachedGrepOutput> {
    match serde_json::from_slice::<CachedGrepOutput>(bytes) {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            let content = String::from_utf8(bytes.to_vec()).ok()?;
            Some(CachedGrepOutput {
                content,
                success: Some(true),
            })
        }
    }
}

#[async_trait]
impl ToolHandler for GrepFilesHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            payload,
            session,
            turn,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "grep_files handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: GrepFilesArgs = serde_json::from_str(&arguments).map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to parse function arguments: {err:?}"
            ))
        })?;

        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "pattern must not be empty".to_string(),
            ));
        }

        if args.limit == 0 {
            return Err(FunctionCallError::RespondToModel(
                "limit must be greater than zero".to_string(),
            ));
        }

        let limit = args.limit.min(MAX_LIMIT);
        let search_path = turn.resolve_path(args.path.clone());

        verify_path_exists(&search_path).await?;

        let include = args.include.as_deref().map(str::trim).and_then(|val| {
            if val.is_empty() {
                None
            } else {
                Some(val.to_string())
            }
        });

        let cache_manager = session.cache_manager();
        let repo_state = if cache_manager.enabled() {
            detect_repo_state(&turn.cwd).await
        } else {
            None
        };
        let cache_key = if cache_manager.enabled() {
            let inputs = GrepCacheKeyInputs {
                workspace_root: &turn.cwd,
                search_path: &search_path,
                pattern,
                include: include.as_deref(),
                limit,
                repo_state: repo_state.as_ref(),
            };
            match build_grep_cache_key(&inputs) {
                Ok(key) => Some(key),
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "failed to compute cache key for grep_files: {err}"
                    );
                    None
                }
            }
        } else {
            None
        };
        let cache_ttl = cache_ttl_for_repo_state(
            cache_manager.ttl_for(CacheableTool::GrepFiles),
            repo_state.as_ref(),
        );

        if let Some(cache_key) = cache_key.as_ref()
            && let Some(cached) = cache_manager.get(cache_key, CacheableTool::GrepFiles)
        {
            if let Some(cached_output) = decode_cached_output(&cached) {
                return Ok(ToolOutput::Function {
                    content: cached_output.content,
                    content_items: None,
                    success: cached_output.success,
                });
            }
            warn!(
                target: LOG_TARGET,
                "failed to decode cached grep_files output: not valid UTF-8"
            );
        }

        let search_results =
            run_rg_search(pattern, include.as_deref(), &search_path, limit, &turn.cwd).await?;

        let (content, success) = if search_results.is_empty() {
            ("No matches found.".to_string(), Some(false))
        } else {
            (search_results.join("\n"), Some(true))
        };

        if let Some(cache_key) = cache_key {
            let cached = CachedGrepOutput {
                content: content.clone(),
                success,
            };
            let encoded = serde_json::to_vec(&cached).unwrap_or_else(|err| {
                warn!(
                    target: LOG_TARGET,
                    "failed to encode grep_files cache entry: {err}"
                );
                content.as_bytes().to_vec()
            });
            cache_manager.put(cache_key, encoded, cache_ttl, CacheableTool::GrepFiles);
        }

        Ok(ToolOutput::Function {
            content,
            content_items: None,
            success,
        })
    }
}

async fn verify_path_exists(path: &Path) -> Result<(), FunctionCallError> {
    tokio::fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to access `{}`: {err}", path.display()))
    })?;
    Ok(())
}

async fn run_rg_search(
    pattern: &str,
    include: Option<&str>,
    search_path: &Path,
    limit: usize,
    cwd: &Path,
) -> Result<Vec<String>, FunctionCallError> {
    let mut command = Command::new("rg");
    command
        .current_dir(cwd)
        .arg("--files-with-matches")
        .arg("--sortr=modified")
        .arg("--regexp")
        .arg(pattern)
        .arg("--no-messages");

    if let Some(glob) = include {
        command.arg("--glob").arg(glob);
    }

    command.arg("--").arg(search_path);

    let output = timeout(COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            FunctionCallError::RespondToModel("rg timed out after 30 seconds".to_string())
        })?
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to launch rg: {err}. Ensure ripgrep is installed and on PATH."
            ))
        })?;

    match output.status.code() {
        Some(0) => Ok(parse_results(&output.stdout, limit)),
        Some(1) => Ok(Vec::new()),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(FunctionCallError::RespondToModel(format!(
                "rg failed: {stderr}"
            )))
        }
    }
}

fn parse_results(stdout: &[u8], limit: usize) -> Vec<String> {
    let mut results = Vec::new();
    for line in stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(line) {
            if text.is_empty() {
                continue;
            }
            results.push(text.to_string());
            if results.len() == limit {
                break;
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::config::DEFAULT_CACHE_GREP_FILES_TTL_SECS;
    use pretty_assertions::assert_eq;
    use std::process::Command as StdCommand;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn parses_basic_results() {
        let stdout = b"/tmp/file_a.rs\n/tmp/file_b.rs\n";
        let parsed = parse_results(stdout, 10);
        assert_eq!(
            parsed,
            vec!["/tmp/file_a.rs".to_string(), "/tmp/file_b.rs".to_string()]
        );
    }

    #[test]
    fn parse_truncates_after_limit() {
        let stdout = b"/tmp/file_a.rs\n/tmp/file_b.rs\n/tmp/file_c.rs\n";
        let parsed = parse_results(stdout, 2);
        assert_eq!(
            parsed,
            vec!["/tmp/file_a.rs".to_string(), "/tmp/file_b.rs".to_string()]
        );
    }

    #[tokio::test]
    async fn run_search_returns_results() -> anyhow::Result<()> {
        if !rg_available() {
            return Ok(());
        }
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();
        std::fs::write(dir.join("match_one.txt"), "alpha beta gamma").unwrap();
        std::fs::write(dir.join("match_two.txt"), "alpha delta").unwrap();
        std::fs::write(dir.join("other.txt"), "omega").unwrap();

        let results = run_rg_search("alpha", None, dir, 10, dir).await?;
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|path| path.ends_with("match_one.txt")));
        assert!(results.iter().any(|path| path.ends_with("match_two.txt")));
        Ok(())
    }

    #[tokio::test]
    async fn run_search_with_glob_filter() -> anyhow::Result<()> {
        if !rg_available() {
            return Ok(());
        }
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();
        std::fs::write(dir.join("match_one.rs"), "alpha beta gamma").unwrap();
        std::fs::write(dir.join("match_two.txt"), "alpha delta").unwrap();

        let results = run_rg_search("alpha", Some("*.rs"), dir, 10, dir).await?;
        assert_eq!(results.len(), 1);
        assert!(results.iter().all(|path| path.ends_with("match_one.rs")));
        Ok(())
    }

    #[tokio::test]
    async fn run_search_respects_limit() -> anyhow::Result<()> {
        if !rg_available() {
            return Ok(());
        }
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();
        std::fs::write(dir.join("one.txt"), "alpha one").unwrap();
        std::fs::write(dir.join("two.txt"), "alpha two").unwrap();
        std::fs::write(dir.join("three.txt"), "alpha three").unwrap();

        let results = run_rg_search("alpha", None, dir, 2, dir).await?;
        assert_eq!(results.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn run_search_handles_no_matches() -> anyhow::Result<()> {
        if !rg_available() {
            return Ok(());
        }
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();
        std::fs::write(dir.join("one.txt"), "omega").unwrap();

        let results = run_rg_search("alpha", None, dir, 5, dir).await?;
        assert!(results.is_empty());
        Ok(())
    }

    #[test]
    fn cached_output_round_trips() {
        let payload = CachedGrepOutput {
            content: "No matches found.".to_string(),
            success: Some(false),
        };
        let encoded = serde_json::to_vec(&payload).expect("encode cache output");

        let decoded = decode_cached_output(&encoded);

        assert!(decoded.is_some());
        let decoded = decoded.expect("decoded");
        assert_eq!(decoded.content, payload.content);
        assert_eq!(decoded.success, payload.success);
    }

    #[tokio::test]
    async fn detects_repo_state_from_git_dir() {
        let workspace = tempdir().expect("tempdir");
        let git_dir = workspace.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(git_dir.join("index"), []).unwrap();

        let state = detect_repo_state(workspace.path()).await;

        assert!(state.is_some());
        let state = state.expect("state");
        assert_eq!(state.head_ref.as_deref(), Some("ref: refs/heads/main"));
        assert!(state.index_mtime_nanos.is_some());
    }

    #[tokio::test]
    async fn detects_repo_state_from_git_file() {
        let workspace = tempdir().expect("tempdir");
        let real_git = workspace.path().join("nested_git");
        std::fs::create_dir_all(&real_git).unwrap();
        std::fs::write(real_git.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        std::fs::write(real_git.join("index"), []).unwrap();
        std::fs::write(
            workspace.path().join(".git"),
            format!("gitdir: {}", real_git.display()),
        )
        .unwrap();

        let state = detect_repo_state(workspace.path()).await;

        assert!(state.is_some());
        let state = state.expect("state");
        assert_eq!(state.head_ref.as_deref(), Some("ref: refs/heads/feature"));
    }

    #[tokio::test]
    async fn builds_cache_key_with_repo_state_changes() {
        let workspace = tempdir().expect("tempdir");
        let search_path = workspace.path().join("search");
        std::fs::create_dir_all(&search_path).unwrap();
        let first = RepoState {
            head_ref: Some("ref: refs/heads/main".to_string()),
            index_mtime_nanos: Some(1),
        };
        let second = RepoState {
            head_ref: Some("ref: refs/heads/feature".to_string()),
            index_mtime_nanos: Some(1),
        };
        let inputs = GrepCacheKeyInputs {
            workspace_root: workspace.path(),
            search_path: &search_path,
            pattern: "alpha",
            include: None,
            limit: 10,
            repo_state: Some(&first),
        };
        let first_key = build_grep_cache_key(&inputs).expect("first key");
        let second_inputs = GrepCacheKeyInputs {
            repo_state: Some(&second),
            ..inputs
        };
        let second_key = build_grep_cache_key(&second_inputs).expect("second key");

        assert_ne!(first_key, second_key);
    }

    #[test]
    fn cache_ttl_falls_back_without_repo_state() {
        let configured = Duration::from_secs(60);
        let ttl = cache_ttl_for_repo_state(configured, None);

        assert_eq!(ttl, Duration::from_secs(DEFAULT_CACHE_GREP_FILES_TTL_SECS));
    }

    fn rg_available() -> bool {
        StdCommand::new("rg")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}
