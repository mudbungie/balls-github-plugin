//! Integration tests for `push --task`.

mod common;
use common::{bin, fnv_hex, write_config, write_token};

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
fn push_patches_when_balls_title_changes_without_status_change() {
    // bl-73cd: balls-side title edit (no status change) must PATCH GH.
    // The projection records "Old [bl-1]" as the last pushed title;
    // the task now carries "New" so pushed title is "New [bl-1]" —
    // status and body match, only the title moved.
    let mut server = mockito::Server::new();
    server
        .mock("PATCH", "/repos/o/n/issues/11")
        .with_status(200)
        .with_body(r#"{"number":11,"html_url":"u","state":"open"}"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    let empty = fnv_hex("");
    let stdin = format!(
        r#"{{"id":"bl-1","title":"New","status":"open",
             "external":{{"github-issues":{{"issue":{{
                 "number":11,"url":"u","state":"open",
                 "source":"balls","synced_at":"t",
                 "last_synced_status":"open",
                 "last_synced_title":"Old [bl-1]",
                 "last_synced_body_hash":"{empty}"}}}}}}}}"#
    );

    bin()
        .args(["push", "--task", "bl-1", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(stdin)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""last_synced_title":"New [bl-1]""#));
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
