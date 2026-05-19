//! Integration tests for `push --task`.

mod common;
use common::{bin, write_config, write_token};

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
        .write_stdin(r#"{"id":"bl-1","title":"Do it","status":"open","description":"body"}"#)
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
