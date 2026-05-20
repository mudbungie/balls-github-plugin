//! Tests for `pull_emit.rs`. Sibling file pattern from the file-
//! decomposition convention.

use super::*;
use crate::pull::GhLabel;

fn cfg(mirror: &str) -> PluginConfig {
    serde_json::from_str(&format!(
        r#"{{"repo":"o/n","close_mirror":"{mirror}"}}"#
    ))
    .unwrap()
}

fn task(json: &str) -> Task {
    serde_json::from_str(json).unwrap()
}

fn issue(state: &str, number: u64) -> GhIssue {
    GhIssue {
        number,
        title: "t".into(),
        body: None,
        state: state.into(),
        html_url: "u".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        labels: Vec::<GhLabel>::new(),
    }
}

#[test]
fn emits_close_when_gh_closed_and_balls_open_authoritative() {
    let t = task(r#"{"id":"bl-1","title":"t","status":"open"}"#);
    let i = issue("closed", 7);
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    assert_eq!(upd.task_id, "bl-1");
    assert_eq!(upd.fields["status"], Value::String("closed".into()));
    assert!(upd.add_note.contains("#7"));
    assert!(upd.add_note.contains("authoritative"));
}

#[test]
fn emits_close_with_best_effort_tag_when_policy_is_best_effort() {
    let t = task(r#"{"id":"bl-2","title":"t","status":"in_progress"}"#);
    let i = issue("closed", 9);
    let upd = updated_from(&i, &t, &cfg("best_effort")).unwrap();
    assert!(upd.add_note.contains("best_effort"));
    assert_eq!(upd.fields["status"], Value::String("closed".into()));
}

#[test]
fn skips_when_close_mirror_off() {
    let t = task(r#"{"id":"bl-3","title":"t","status":"open"}"#);
    let i = issue("closed", 11);
    assert!(updated_from(&i, &t, &cfg("off")).is_none());
}

#[test]
fn skips_when_gh_open() {
    let t = task(r#"{"id":"bl-4","title":"t","status":"open"}"#);
    let i = issue("open", 5);
    assert!(updated_from(&i, &t, &cfg("authoritative")).is_none());
}

#[test]
fn skips_when_balls_already_closed() {
    // The mirror has already converged; emitting again would loop.
    let t = task(r#"{"id":"bl-5","title":"t","status":"closed"}"#);
    let i = issue("closed", 13);
    assert!(updated_from(&i, &t, &cfg("authoritative")).is_none());
}

#[test]
fn close_mirror_tag_round_trips_all_variants() {
    // updated_from only reaches close_mirror_tag with non-Off
    // variants (Off returns early), but the tag function is part of
    // the public-ish surface for future call sites and must spell
    // every variant. Calling it directly keeps each arm exercised.
    assert_eq!(close_mirror_tag(CloseMirror::Authoritative), "authoritative");
    assert_eq!(close_mirror_tag(CloseMirror::BestEffort), "best_effort");
    assert_eq!(close_mirror_tag(CloseMirror::Off), "off");
}
