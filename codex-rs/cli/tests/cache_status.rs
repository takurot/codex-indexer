use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn cache_status_reports_breakdown_by_tool() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;

    let output = Command::cargo_bin("codex")?
        .env("CODEX_HOME", codex_home.path())
        .args(["cache", "status", "--by-tool"])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("read_file"),
        "stdout missing read_file: {stdout}"
    );
    assert!(
        stdout.contains("list_dir"),
        "stdout missing list_dir: {stdout}"
    );
    assert!(
        stdout.contains("grep_files"),
        "stdout missing grep_files: {stdout}"
    );

    Ok(())
}
