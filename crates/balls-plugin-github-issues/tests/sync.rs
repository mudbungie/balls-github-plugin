//! Integration tests for `sync`.

mod common;
use common::{bin, write_config, write_config_with_label, write_token};

#[test]
fn sync_with_empty_repo_emits_empty_report() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body("[]")
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
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
fn sync_accepts_empty_stdin_as_empty_task_list() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body("[]")
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    bin()
        .args(["sync", "--task", "bl-x", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicates::str::starts_with("{}"));
}

#[test]
fn sync_label_filter_skips_non_matching_issues() {
    // Issue lacks the configured target_label; classify yields
    // Skip(LabelFilter); sync emits an empty report. Exercises the
    // Skip arm of the sync match.
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":1,"title":"Off-label","state":"open","html_url":"u",
                 "updated_at":"2026-01-01T00:00:00Z","labels":[{"name":"other"}]}]"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config_with_label(dir.path(), &server.url(), "balls:track");
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
fn sync_auto_creates_new_balls_task_for_unmatched_gh_issue() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":99,"title":"External report","state":"open","html_url":"https://gh/i/99",
                 "updated_at":"2026-01-01T00:00:00Z","body":"details","labels":[{"name":"bug"}]}]"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("[]")
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""title":"External report""#))
        .stdout(predicates::str::contains(r#""source":"github""#))
        .stdout(predicates::str::contains(r#""bug""#));
}

#[test]
fn sync_emits_deferred_when_gh_issue_vanishes() {
    // GH list returns no issues; a balls task references issue #5
    // by stored number. Default on_external_delete=deferred ->
    // emit updated with status=deferred.
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body("[]")
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    let tasks = r#"[{"id":"bl-gone","title":"Was tracked","status":"open",
        "external":{"github_issues":{"issue":{
            "number":5,"url":"u","state":"open",
            "source":"balls","synced_at":"2026-01-01T00:00:00+00:00",
            "last_synced_status":"open"}}}}]"#;

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""task_id":"bl-gone""#))
        .stdout(predicates::str::contains(r#""status":"deferred""#))
        .stdout(predicates::str::contains("no longer found"));
}

#[test]
fn sync_emits_close_for_gh_closed_known_issue() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":5,"title":"Track [bl-aaaa]","state":"closed","html_url":"u",
                 "updated_at":"2026-01-02T00:00:00Z","labels":[]}]"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    let tasks = r#"[{"id":"bl-aaaa","title":"Track","status":"open",
        "external":{"github_issues":{"issue":{
            "number":5,"url":"u","state":"open",
            "source":"balls","synced_at":"2026-01-01T00:00:00+00:00",
            "last_synced_status":"open"}}}}]"#;

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""task_id":"bl-aaaa""#))
        .stdout(predicates::str::contains(r#""status":"closed""#))
        .stdout(predicates::str::contains("closed externally"));
}
