use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use codex_common::CliConfigOverrides;
use codex_core::AuthManager;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::semantic::index::SearchHit;
use codex_core::semantic::index::SemanticIndex;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Parser)]
pub(crate) struct SearchCommand {
    /// Search query string (wrap in quotes for spaces).
    #[arg(value_name = "QUERY", num_args = 1..)]
    pub(crate) query: Vec<String>,

    /// Number of top matches to return (defaults to config).
    #[arg(long, value_name = "N")]
    pub(crate) topk: Option<usize>,

    /// Output results as JSON.
    #[arg(long)]
    pub(crate) json: bool,

    #[clap(flatten)]
    pub(crate) config_overrides: CliConfigOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnippetLine {
    line_number: usize,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
struct SearchResult {
    file_path: String,
    start_line: usize,
    end_line: usize,
    score: f32,
    snippet: Vec<SnippetLine>,
    snippet_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResultsJson {
    query: String,
    top_k: usize,
    results: Vec<SearchResultJson>,
}

#[derive(Debug, Serialize)]
struct SearchResultJson {
    file_path: String,
    start_line: usize,
    end_line: usize,
    score: f32,
    snippet: Vec<SnippetLineJson>,
    snippet_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SnippetLineJson {
    line_number: usize,
    text: String,
}

pub(crate) async fn run_search_command(cmd: SearchCommand) -> Result<()> {
    let query = cmd.query.join(" ").trim().to_string();
    if query.is_empty() {
        anyhow::bail!("search query cannot be empty");
    }

    let cli_overrides = cmd
        .config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let config = Config::load_with_cli_overrides_and_harness_overrides(
        cli_overrides,
        ConfigOverrides::default(),
    )
    .await?;

    let auth_manager = Arc::new(AuthManager::new(
        config.codex_home.clone(),
        false,
        config.cli_auth_credentials_store_mode,
    ));
    let index = SemanticIndex::new(
        config.cwd.clone(),
        config.semantic_index.clone(),
        config.model_provider.clone(),
        Some(auth_manager),
    );

    let top_k = cmd.topk.unwrap_or(config.semantic_index.retrieve.top_k);
    let hits = index.search(&query, top_k).await?;
    let results = build_search_results(
        config.cwd.as_path(),
        hits,
        config.semantic_index.retrieve.max_chars,
    );

    if cmd.json {
        let output = SearchResultsJson {
            query,
            top_k,
            results: results.into_iter().map(SearchResultJson::from).collect(),
        };
        let payload = serde_json::to_string_pretty(&output)?;
        println!("{payload}");
        return Ok(());
    }

    for line in format_search_results(&results) {
        println!("{line}");
    }

    Ok(())
}

fn build_search_results(
    workspace_root: &Path,
    hits: Vec<SearchHit>,
    max_chars: usize,
) -> Vec<SearchResult> {
    hits.into_iter()
        .map(|hit| {
            let file_path = hit.file_path.clone();
            let full_path = workspace_root.join(&file_path);
            let snippet_result =
                read_snippet_lines(&full_path, hit.start_line, hit.end_line, max_chars);
            let (snippet, snippet_error) = match snippet_result {
                Ok(lines) => (lines, None),
                Err(err) => (Vec::new(), Some(err.to_string())),
            };
            SearchResult {
                file_path,
                start_line: hit.start_line,
                end_line: hit.end_line,
                score: hit.score,
                snippet,
                snippet_error,
            }
        })
        .collect()
}

fn read_snippet_lines(
    path: &Path,
    start_line: usize,
    end_line: usize,
    max_chars: usize,
) -> Result<Vec<SnippetLine>> {
    let path_display = path.display();
    let bytes = fs::read(path).with_context(|| format!("failed to read {path_display}"))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let contents = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    let start = start_line.max(1);
    let end = end_line.max(start);
    let mut remaining = if max_chars == 0 {
        usize::MAX
    } else {
        max_chars
    };

    for (idx, line) in contents.lines().enumerate() {
        let line_number = idx + 1;
        if line_number < start {
            continue;
        }
        if line_number > end {
            break;
        }
        if remaining == 0 && !out.is_empty() {
            break;
        }
        let text = if remaining == usize::MAX || line.len() <= remaining {
            line.to_string()
        } else {
            line.chars().take(remaining).collect()
        };
        if remaining != usize::MAX {
            remaining = remaining.saturating_sub(text.len());
        }
        out.push(SnippetLine { line_number, text });
        if remaining == 0 {
            break;
        }
    }

    Ok(out)
}

fn format_search_results(results: &[SearchResult]) -> Vec<String> {
    let mut lines = Vec::new();
    if results.is_empty() {
        lines.push("No results found.".to_string());
        return lines;
    }
    for result in results {
        let file_path = &result.file_path;
        let start_line = result.start_line;
        let end_line = result.end_line;
        let score = result.score;
        lines.push(format!(
            "{file_path}:{start_line}-{end_line} score={score:.3}"
        ));
        if result.snippet.is_empty() {
            let message = result
                .snippet_error
                .as_deref()
                .map(|err| format!("(snippet unavailable: {err})"))
                .unwrap_or_else(|| "(no snippet)".to_string());
            lines.push(format!("  {message}"));
            continue;
        }
        let width = result.end_line.to_string().len().max(1);
        for snippet_line in &result.snippet {
            let line_number = snippet_line.line_number;
            let text = &snippet_line.text;
            lines.push(format!("  {line_number:>width$} | {text}"));
        }
    }
    lines
}

impl From<SearchResult> for SearchResultJson {
    fn from(result: SearchResult) -> Self {
        Self {
            file_path: result.file_path,
            start_line: result.start_line,
            end_line: result.end_line,
            score: result.score,
            snippet: result
                .snippet
                .into_iter()
                .map(|line| SnippetLineJson {
                    line_number: line.line_number,
                    text: line.text,
                })
                .collect(),
            snippet_error: result.snippet_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn read_snippet_lines_truncates_to_max_chars() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("sample.txt");
        fs::write(&path, "abcdef\n")?;

        let lines = read_snippet_lines(&path, 1, 1, 3)?;
        let expected = vec![SnippetLine {
            line_number: 1,
            text: "abc".to_string(),
        }];
        assert_eq!(lines, expected);
        Ok(())
    }

    #[test]
    fn format_search_results_includes_line_range_and_snippet() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("sample.rs");
        fs::write(&path, "one\ntwo\nthree\n")?;

        let hit = SearchHit {
            file_path: "sample.rs".to_string(),
            start_line: 2,
            end_line: 3,
            score: 0.42,
            chunk_id: "chunk-1".to_string(),
        };
        let results = build_search_results(dir.path(), vec![hit], 1024);
        let rendered = format_search_results(&results);

        assert_eq!(
            rendered,
            vec![
                "sample.rs:2-3 score=0.420".to_string(),
                "  2 | two".to_string(),
                "  3 | three".to_string(),
            ]
        );
        Ok(())
    }
}
