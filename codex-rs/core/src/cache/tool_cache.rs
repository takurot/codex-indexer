use serde_json::Value as JsonValue;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathStamp {
    pub mtime_nanos: u128,
    pub size_bytes: u64,
}

pub fn build_tool_cache_key(
    tool_name: &str,
    args: &JsonValue,
    workspace_root: &Path,
    target_path: &Path,
    stamp: PathStamp,
) -> std::io::Result<String> {
    let canonical_args = canonical_json(args);
    let serialized_args = serde_json::to_string(&canonical_args).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to serialize cache arguments: {err}"),
        )
    })?;
    let workspace = normalize_path(workspace_root);
    let target = normalize_path(target_path);
    let raw_key = format!(
        "{tool_name}|{serialized_args}|{workspace}|{target}|{mtime_nanos}|{size_bytes}",
        mtime_nanos = stamp.mtime_nanos,
        size_bytes = stamp.size_bytes
    );
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let hash = hasher.finalize();
    let hex = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(hex)
}

pub async fn build_tool_cache_key_for_path(
    tool_name: &str,
    args: &JsonValue,
    workspace_root: &Path,
    target_path: &Path,
) -> std::io::Result<String> {
    let metadata = tokio::fs::metadata(target_path).await?;
    let stamp = stamp_from_metadata(&metadata)?;
    build_tool_cache_key(tool_name, args, workspace_root, target_path, stamp)
}

pub fn stamp_from_metadata(metadata: &std::fs::Metadata) -> std::io::Result<PathStamp> {
    let mtime = metadata.modified().unwrap_or(UNIX_EPOCH);
    let duration = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(PathStamp {
        mtime_nanos: duration.as_nanos(),
        size_bytes: metadata.len(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::Map;
    use std::path::Path;

    #[test]
    fn canonical_json_sorts_keys() {
        let mut map_a = Map::new();
        map_a.insert("b".to_string(), JsonValue::Bool(true));
        map_a.insert("a".to_string(), JsonValue::Number(1.into()));
        let mut map_b = Map::new();
        map_b.insert("a".to_string(), JsonValue::Number(1.into()));
        map_b.insert("b".to_string(), JsonValue::Bool(true));

        let value_a = JsonValue::Object(map_a);
        let value_b = JsonValue::Object(map_b);
        let stamp = PathStamp {
            mtime_nanos: 1,
            size_bytes: 2,
        };
        let key_a = build_tool_cache_key(
            "read_file",
            &value_a,
            Path::new("/tmp"),
            Path::new("/tmp/a"),
            stamp,
        )
        .expect("key a");
        let key_b = build_tool_cache_key(
            "read_file",
            &value_b,
            Path::new("/tmp"),
            Path::new("/tmp/a"),
            stamp,
        )
        .expect("key b");

        assert_eq!(key_a, key_b);
    }

    #[test]
    fn cache_key_changes_with_path_stamp() {
        let args = serde_json::json!({"file_path":"/tmp/a","offset":1});
        let key_a = build_tool_cache_key(
            "read_file",
            &args,
            Path::new("/tmp"),
            Path::new("/tmp/a"),
            PathStamp {
                mtime_nanos: 10,
                size_bytes: 20,
            },
        )
        .expect("key a");
        let key_b = build_tool_cache_key(
            "read_file",
            &args,
            Path::new("/tmp"),
            Path::new("/tmp/a"),
            PathStamp {
                mtime_nanos: 11,
                size_bytes: 20,
            },
        )
        .expect("key b");

        assert_ne!(key_a, key_b);
    }
}
