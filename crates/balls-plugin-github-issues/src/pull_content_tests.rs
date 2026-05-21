//! Tests for `pull_content.rs`.

use super::*;
use crate::pull::GhLabel;

fn task(json: &str) -> Task {
    serde_json::from_str(json).unwrap()
}

fn issue(title: &str, body: Option<&str>) -> GhIssue {
    GhIssue {
        number: 1,
        title: title.into(),
        body: body.map(str::to_string),
        state: "open".into(),
        html_url: "u".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        labels: Vec::<GhLabel>::new(),
        pull_request: None,
    }
}

#[test]
fn body_hash_is_deterministic_and_constant_size() {
    let h1 = body_hash("hello world");
    let h2 = body_hash("hello world");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 16);
    let h3 = body_hash("hello world!");
    assert_ne!(h1, h3);
    assert_eq!(body_hash("").len(), 16);
}

#[test]
fn pushed_title_includes_marker() {
    let t = task(r#"{"id":"bl-1234","title":"My task","status":"open"}"#);
    assert_eq!(pushed_title(&t), "My task [bl-1234]");
}

#[test]
fn strip_marker_removes_trailing_marker() {
    assert_eq!(strip_marker("Foo [bl-abcd]", "bl-abcd"), "Foo");
}

#[test]
fn strip_marker_leaves_title_when_marker_absent_or_wrong_id() {
    assert_eq!(strip_marker("Foo", "bl-abcd"), "Foo");
    assert_eq!(strip_marker("Foo [bl-other]", "bl-abcd"), "Foo [bl-other]");
    // Marker not at the end stays put — we only strip a clean suffix.
    assert_eq!(
        strip_marker("Foo [bl-abcd] trailing", "bl-abcd"),
        "Foo [bl-abcd] trailing"
    );
}

#[test]
fn decide_title_noop_when_neither_moved() {
    let t = task(r#"{"id":"bl-1","title":"X","status":"open"}"#);
    let i = issue("X [bl-1]", None);
    assert_eq!(decide_title(&i, &t, "X [bl-1]"), FieldDecision::Noop);
}

#[test]
fn decide_title_noop_when_only_balls_moved() {
    // last_synced records old GH-side value; GH still has the old
    // value; balls changed its title locally. Pull is a noop — the
    // next push will sync balls's new title to GH.
    let t = task(r#"{"id":"bl-1","title":"Updated","status":"open"}"#);
    let i = issue("Old [bl-1]", None);
    assert_eq!(decide_title(&i, &t, "Old [bl-1]"), FieldDecision::Noop);
}

#[test]
fn decide_title_mirror_when_only_gh_moved() {
    let t = task(r#"{"id":"bl-1","title":"Original","status":"open"}"#);
    let i = issue("Renamed in GH [bl-1]", None);
    assert_eq!(
        decide_title(&i, &t, "Original [bl-1]"),
        FieldDecision::Mirror
    );
}

#[test]
fn decide_title_conflict_when_both_moved() {
    let t = task(r#"{"id":"bl-1","title":"Balls renamed","status":"open"}"#);
    let i = issue("GH renamed [bl-1]", None);
    assert_eq!(
        decide_title(&i, &t, "Original [bl-1]"),
        FieldDecision::Conflict
    );
}

#[test]
fn decide_body_noop_when_neither_moved() {
    let t = task(
        r#"{"id":"bl-1","title":"X","status":"open","description":"body"}"#,
    );
    let i = issue("X", Some("body"));
    let h = body_hash("body");
    assert_eq!(decide_body(&i, &t, &h), FieldDecision::Noop);
}

#[test]
fn decide_body_mirror_when_only_gh_moved() {
    let t = task(
        r#"{"id":"bl-1","title":"X","status":"open","description":"old"}"#,
    );
    let i = issue("X", Some("new body from gh"));
    let h = body_hash("old");
    assert_eq!(decide_body(&i, &t, &h), FieldDecision::Mirror);
}

#[test]
fn decide_body_conflict_when_both_moved() {
    let t = task(
        r#"{"id":"bl-1","title":"X","status":"open","description":"balls edit"}"#,
    );
    let i = issue("X", Some("gh edit"));
    let h = body_hash("original");
    assert_eq!(decide_body(&i, &t, &h), FieldDecision::Conflict);
}

#[test]
fn decide_body_treats_missing_gh_body_as_empty() {
    let t = task(r#"{"id":"bl-1","title":"X","status":"open"}"#);
    let i = issue("X", None);
    // task.description defaults to "" on Task; body_hash("") matches.
    let h = body_hash("");
    assert_eq!(decide_body(&i, &t, &h), FieldDecision::Noop);
}
