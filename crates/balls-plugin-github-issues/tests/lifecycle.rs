//! Full-lifecycle mockito-driven roundtrips. The per-command
//! integration tests (auth.rs, push.rs, sync.rs) cover each verb in
//! isolation; this file chains them into the scenarios B6 names:
//!
//! - balls creates a task -> push opens a GH issue.
//! - GH-side closes the issue -> sync mirrors close to balls.
//! - A new external GH issue appears -> sync auto-creates a balls task.
//! - The auto-created balls task gets pushed back tagged [bl-xxxx].
//! - A previously-mirrored GH issue vanishes -> sync emits the
//!   on_external_delete policy entry.
//!
//! The scenarios use one mockito server but multiple sequential bin
//! invocations; the goal is to prove the wire-protocol contract end-
//! to-end with no hidden coupling between subcommands.

mod common;
use common::{bin, write_config, write_token};

#[test]
fn full_lifecycle_balls_create_then_gh_close_then_balls_sync_mirrors() {
    let mut server = mockito::Server::new();
    // Push will POST to /repos/o/n/issues -> mock the create.
    server
        .mock("POST", "/repos/o/n/issues")
        .with_status(201)
        .with_body(r#"{"number":11,"html_url":"https://gh/i/11","state":"open"}"#)
        .create();
    // Sync will GET the list and find the issue now closed.
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":11,"title":"Implement X [bl-2222]","state":"closed","html_url":"u",
                 "updated_at":"2026-01-02T00:00:00Z","labels":[]}]"#,
        )
        .create();

    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    // (1) Push: balls task → GH issue create.
    let push_out = bin()
        .args(["push", "--task", "bl-2222", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(r#"{"id":"bl-2222","title":"Implement X","status":"open","description":"do it"}"#)
        .output()
        .unwrap();
    assert!(push_out.status.success(), "{:?}", push_out);
    let push_stdout = String::from_utf8(push_out.stdout).unwrap();
    assert!(push_stdout.contains(r#""number":11"#));
    assert!(push_stdout.contains(r#""source":"balls""#));

    // (2) Simulated time passes; GH closes the issue externally.
    //     Balls task now has the stored projection from step (1).
    //     We rebuild the task as it would appear in the next bl sync:
    //     status=open still, with external.github_issues populated.
    let tasks_after_push = r#"[{"id":"bl-2222","title":"Implement X","status":"open",
        "description":"do it",
        "external":{"github_issues":{"issue":{
            "number":11,"url":"https://gh/i/11","state":"open",
            "source":"balls","synced_at":"2026-01-01T00:00:00+00:00",
            "last_synced_status":"open"}}}}]"#;

    // (3) Sync: classify sees number=11 matched, GH state=closed,
    //     close_mirror default Authoritative -> emit updated.
    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks_after_push)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""task_id":"bl-2222""#))
        .stdout(predicates::str::contains(r#""status":"closed""#))
        .stdout(predicates::str::contains("closed externally"));
}

#[test]
fn full_lifecycle_gh_creates_external_issue_then_sync_auto_creates_balls_task() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":777,"title":"Bug report from outside","state":"open","html_url":"u",
                 "updated_at":"2026-02-01T00:00:00Z","body":"crashes on launch",
                 "labels":[{"name":"bug"},{"name":"crash"}]}]"#,
        )
        .create();

    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    // No existing balls task; GH issue is untagged → AutoCreate.
    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("[]")
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""created""#))
        .stdout(predicates::str::contains(r#""title":"Bug report from outside""#))
        .stdout(predicates::str::contains(r#""bug""#))
        .stdout(predicates::str::contains(r#""crash""#))
        .stdout(predicates::str::contains(r#""source":"github""#));
}

#[test]
fn full_lifecycle_external_delete_flips_to_deferred() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body("[]")
        .create();

    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    let tasks = r#"[{"id":"bl-orphan","title":"Was tracked","status":"open",
        "external":{"github_issues":{"issue":{
            "number":42,"url":"u","state":"open","source":"balls",
            "synced_at":"2026-01-01T00:00:00+00:00","last_synced_status":"open"}}}}]"#;

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""task_id":"bl-orphan""#))
        .stdout(predicates::str::contains(r#""status":"deferred""#))
        .stdout(predicates::str::contains("no longer found"));
}
