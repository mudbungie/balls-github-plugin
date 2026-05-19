//! End-to-end tests for `balls-plugin-github-issues`. B1 only
//! exercises the binary's wire-up: every command dispatches, auth
//! works against a mock GitHub, push and sync are silent noops.
//! Subsequent B-children replace those noops with real behavior and
//! add cases here as they land.

use assert_cmd::Command;
use std::path::Path;

fn write_config(dir: &Path, api_base: &str) -> std::path::PathBuf {
    let p = dir.join("github-issues.json");
    std::fs::write(
        &p,
        format!(r#"{{"repo":"o/n","api_base":"{}"}}"#, api_base),
    )
    .unwrap();
    p
}

fn write_token(dir: &Path) {
    std::fs::write(dir.join("token.json"), r#"{"token":"t"}"#).unwrap();
}

fn bin() -> Command {
    Command::cargo_bin("balls-plugin-github-issues").unwrap()
}

#[test]
fn auth_setup_reads_token_from_stdin() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/user")
        .with_status(200)
        .with_body(r#"{"login":"octocat"}"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());

    bin()
        .args(["auth-setup", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("ghp_fromstdin\n")
        .assert()
        .success();

    let stored = std::fs::read_to_string(dir.path().join("token.json")).unwrap();
    assert!(stored.contains("ghp_fromstdin"));
}

#[test]
fn auth_check_succeeds_with_valid_token() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/user")
        .with_status(200)
        .with_body(r#"{"login":"x"}"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    bin()
        .args(["auth-check", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn missing_token_errors_and_exits_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com");

    bin()
        .args(["auth-check", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("error:"));
}

#[test]
fn push_creates_issue_for_open_task() {
    let mut server = mockito::Server::new();
    server
        .mock("POST", "/repos/o/n/issues")
        .with_status(201)
        .with_body(r#"{"number":42,"html_url":"https://gh/i/42","state":"open"}"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    bin()
        .args(["push", "--task", "bl-1", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(
            r#"{"id":"bl-1","title":"Do it","status":"open","description":"body"}"#,
        )
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""number":42"#))
        .stdout(predicates::str::contains(r#""source":"balls""#));
}

#[test]
fn push_noop_for_closed_without_stored_number() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com");
    write_token(dir.path());

    bin()
        .args(["push", "--task", "bl-1", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(r#"{"id":"bl-1","title":"t","status":"closed"}"#)
        .assert()
        .success()
        .stdout(predicates::str::starts_with("{}"));
}

#[test]
fn push_rejects_task_id_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com");
    write_token(dir.path());

    bin()
        .args(["push", "--task", "bl-1", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(r#"{"id":"bl-2","title":"t","status":"open"}"#)
        .assert()
        .failure()
        .stderr(predicates::str::contains("does not match"));
}

#[test]
fn sync_noop_prints_empty_report() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com");
    write_token(dir.path());

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("[]")
        .assert()
        .success()
        .stdout(predicates::str::starts_with("{}"));
}

#[test]
fn sync_with_task_filter_still_noop() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com");
    write_token(dir.path());

    bin()
        .args(["sync", "--task", "bl-x", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("[]")
        .assert()
        .success()
        .stdout(predicates::str::starts_with("{}"));
}
