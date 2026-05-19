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

fn issue_full(state: &str, number: u64, title: &str, body: &str, labels: &[&str]) -> GhIssue {
    GhIssue {
        number,
        title: title.into(),
        body: Some(body.into()),
        state: state.into(),
        html_url: "u".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        labels: labels.iter().map(|n| GhLabel { name: (*n).into() }).collect(),
    }
}

#[test]
fn created_from_simple_issue() {
    let i = issue_full("open", 7, "External report", "short body", &["bug"]);
    let c = created_from(&i);
    assert_eq!(c.title, "External report");
    assert_eq!(c.task_type, "task");
    assert_eq!(c.priority, 3);
    assert_eq!(c.status, "open");
    assert_eq!(c.description, "short body");
    assert_eq!(c.tags, vec!["bug".to_string()]);
    let gh = c.external.get("github_issues").unwrap();
    assert_eq!(gh["issue"]["number"], 7);
    assert_eq!(gh["issue"]["source"], "github");
}

#[test]
fn created_from_truncates_oversized_body() {
    let big = "x".repeat(MAX_BODY_BYTES + 1024);
    let i = issue_full("open", 1, "Big", &big, &[]);
    let c = created_from(&i);
    assert!(c.description.len() < big.len());
    assert!(c.description.contains("truncated to"));
}

#[test]
fn created_from_caps_label_count() {
    let labels: Vec<String> = (0..MAX_LABELS + 10).map(|i| format!("l{i}")).collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let i = issue_full("open", 2, "Many labels", "", &label_refs);
    let c = created_from(&i);
    assert_eq!(c.tags.len(), MAX_LABELS);
    assert_eq!(c.tags[0], "l0");
    assert!(c.description.contains("label set truncated"));
}

#[test]
fn created_from_empty_body_and_labels() {
    let i = issue_full("open", 3, "Simple", "", &[]);
    let c = created_from(&i);
    assert_eq!(c.description, "");
    assert!(c.tags.is_empty());
}

#[test]
fn created_from_handles_utf8_truncation_boundary() {
    // Build a body whose MAX_BODY_BYTES'th byte lands inside a
    // multi-byte UTF-8 sequence; truncation must back up to a char
    // boundary rather than slice mid-codepoint.
    let mut body = "x".repeat(MAX_BODY_BYTES - 1);
    body.push('\u{1F600}'); // 4-byte emoji, straddles the cap
    let i = issue_full("open", 4, "Emoji", &body, &[]);
    let c = created_from(&i);
    assert!(c.description.contains("truncated"));
}
