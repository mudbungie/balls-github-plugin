//! End-to-end tests: run the actual binary so `main` and the stdin-reading
//! `run` wrappers are exercised (and counted by coverage), driven against a
//! mock GitHub API.

use assert_cmd::Command;
use std::path::Path;

fn write_config(dir: &Path, api_base: &str, target: &str) -> std::path::PathBuf {
    let p = dir.join("github.json");
    std::fs::write(
        &p,
        format!(
            r#"{{"repo":"o/n","target_branch":"{}","api_base":"{}"}}"#,
            target, api_base
        ),
    )
    .unwrap();
    p
}

fn write_token(dir: &Path) {
    std::fs::write(dir.join("token.json"), r#"{"token":"t"}"#).unwrap();
}

fn bin() -> Command {
    Command::cargo_bin("balls-plugin-github").unwrap()
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
    let cfg = write_config(dir.path(), &server.url(), "main");

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
    let cfg = write_config(dir.path(), &server.url(), "main");
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
    let cfg = write_config(dir.path(), "https://api.github.com", "main");

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
fn push_opens_a_pr_and_prints_response() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/repos/o/n/pulls")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body("[]")
        .create();
    server
        .mock("POST", "/repos/o/n/pulls")
        .with_status(201)
        .with_body(
            r#"{"number":11,"html_url":"https://gh/pr/11",
                "head":{"ref":"work/bl-1","sha":"s11"},"base":{"ref":"main"}}"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url(), "main");
    write_token(dir.path());

    bin()
        .args(["push", "--task", "bl-1", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(r#"{"id":"bl-1","title":"Do it","status":"review"}"#)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""pull_request""#))
        .stdout(predicates::str::contains(r#""number":11"#));
}

#[test]
fn push_rejects_task_id_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com", "main");
    write_token(dir.path());

    bin()
        .args(["push", "--task", "bl-1", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(r#"{"id":"bl-2","title":"t","status":"review"}"#)
        .assert()
        .failure()
        .stderr(predicates::str::contains("does not match"));
}

#[test]
fn sync_closes_gate_child_when_pr_merged() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/repos/o/n/pulls/7")
        .with_status(200)
        .with_body(
            r#"{"number":7,"html_url":"u","head":{"ref":"h","sha":"z"},
                "base":{"ref":"main"},"merged":true,"merge_commit_sha":"cafe"}"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url(), "main");
    write_token(dir.path());

    let tasks = r#"[{"id":"bl-p","title":"t","status":"review",
        "links":[{"link_type":"gates","target":"bl-g"}],
        "external":{"github":{"pull_request":{"number":7}}}},
        {"id":"bl-g","title":"gate","status":"open"}]"#;

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""task_id":"bl-g""#))
        .stdout(predicates::str::contains("cafe"));
}
