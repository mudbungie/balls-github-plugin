//! bl-4918 content-mirror integration tests. Split out of
//! `tests/sync.rs` to keep each test crate file under the 300-line
//! workspace cap.

mod common;
use common::{bin, fnv_hex, write_config, write_token};
use predicates::prelude::PredicateBooleanExt;

#[test]
fn sync_mirrors_gh_title_edit_when_only_gh_moved() {
    // GH-side title edit on a known issue, balls hasn't touched its
    // title, last_synced_title says "Track [bl-aaaa]".
    // updated_from -> SyncReport.updated with fields.title set, plus
    // a projection refresh whose last_synced_title advances to the
    // new GH-side full title.
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":5,"title":"Renamed in GH [bl-aaaa]","state":"open","html_url":"u",
                 "updated_at":"2026-02-01T00:00:00Z","body":"same body","labels":[]}]"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    let hash = fnv_hex("same body");
    let tasks = format!(
        r#"[{{"id":"bl-aaaa","title":"Track","status":"open","description":"same body",
            "external":{{"github-issues":{{"issue":{{
                "number":5,"url":"u","state":"open","source":"balls",
                "synced_at":"2026-01-01T00:00:00+00:00",
                "last_synced_status":"open",
                "last_synced_title":"Track [bl-aaaa]",
                "last_synced_body_hash":"{hash}"}}}}}}}}]"#
    );

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks)
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""task_id":"bl-aaaa""#))
        .stdout(predicates::str::contains(r#""title":"Renamed in GH""#))
        .stdout(predicates::str::contains("title mirrored"))
        .stdout(predicates::str::contains(
            r#""last_synced_title":"Renamed in GH [bl-aaaa]""#,
        ));
}

#[test]
fn sync_emits_conflict_note_when_both_sides_edited_title() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":5,"title":"GH renamed [bl-aaaa]","state":"open","html_url":"u",
                 "updated_at":"2026-02-01T00:00:00Z","body":"b","labels":[]}]"#,
        )
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    let hash = fnv_hex("b");
    // last_synced_title says "Original" — both GH and balls moved.
    let tasks = format!(
        r#"[{{"id":"bl-aaaa","title":"Balls renamed","status":"open","description":"b",
            "external":{{"github-issues":{{"issue":{{
                "number":5,"url":"u","state":"open","source":"balls",
                "synced_at":"2026-01-01T00:00:00+00:00",
                "last_synced_status":"open",
                "last_synced_title":"Original [bl-aaaa]",
                "last_synced_body_hash":"{hash}"}}}}}}}}]"#
    );

    bin()
        .args(["sync", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin(tasks)
        .assert()
        .success()
        .stdout(predicates::str::contains("title conflict"))
        .stdout(predicates::str::contains(r#""title":"GH renamed"#).not());
}
