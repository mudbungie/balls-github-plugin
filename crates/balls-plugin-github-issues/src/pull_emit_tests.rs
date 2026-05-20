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
    // Per the PushResponse contract, SyncCreate.external is what
    // core inserts verbatim under the participant name. The plugin
    // emits just {"issue": {...}} — no inner participant wrapper.
    let issue = c.external.get("issue").unwrap();
    assert_eq!(issue["number"], 7);
    assert_eq!(issue["source"], "github");
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

fn cfg_with(field: &str) -> PluginConfig {
    serde_json::from_str(&format!(r#"{{"repo":"o/n",{field}}}"#)).unwrap()
}

#[test]
fn deleted_from_deferred_default() {
    let t = task(
        r#"{"id":"bl-1","title":"t","status":"open",
            "external":{"github-issues":{"issue":{
                "number":7,"url":"u","state":"open","source":"balls",
                "synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let cfg = cfg_with(r#""on_external_delete":"deferred""#);
    let upd = deleted_from(&t, &cfg).unwrap();
    assert_eq!(upd.fields["status"], Value::String("deferred".into()));
    assert!(upd.add_note.contains("no longer found"));
    assert!(upd.add_note.contains("deferred"));
}

#[test]
fn deleted_from_closed_policy() {
    let t = task(
        r#"{"id":"bl-2","title":"t","status":"in_progress",
            "external":{"github-issues":{"issue":{
                "number":7,"url":"u","state":"open","source":"balls",
                "synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let cfg = cfg_with(r#""on_external_delete":"closed""#);
    let upd = deleted_from(&t, &cfg).unwrap();
    assert_eq!(upd.fields["status"], Value::String("closed".into()));
}

#[test]
fn deleted_from_noop_returns_none() {
    let t = task(
        r#"{"id":"bl-3","title":"t","status":"open",
            "external":{"github-issues":{"issue":{
                "number":7,"url":"u","state":"open","source":"balls",
                "synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let cfg = cfg_with(r#""on_external_delete":"noop""#);
    assert!(deleted_from(&t, &cfg).is_none());
}

#[test]
fn deleted_from_already_at_target_status_skips() {
    let t = task(
        r#"{"id":"bl-4","title":"t","status":"deferred",
            "external":{"github-issues":{"issue":{
                "number":7,"url":"u","state":"open","source":"balls",
                "synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let cfg = cfg_with(r#""on_external_delete":"deferred""#);
    assert!(deleted_from(&t, &cfg).is_none());
}

#[test]
fn deleted_from_no_stored_number_skips() {
    let t = task(r#"{"id":"bl-5","title":"t","status":"open"}"#);
    let cfg = cfg_with(r#""on_external_delete":"deferred""#);
    assert!(deleted_from(&t, &cfg).is_none());
}

#[test]
fn on_external_delete_tag_round_trips_all_variants() {
    assert_eq!(on_external_delete_tag(OnExternalDelete::Deferred), "deferred");
    assert_eq!(on_external_delete_tag(OnExternalDelete::Closed), "closed");
    assert_eq!(on_external_delete_tag(OnExternalDelete::Noop), "noop");
}

#[test]
fn sweep_deletes_respects_cap_and_skips_present_issues() {
    let t1 = task(
        r#"{"id":"bl-a","title":"a","status":"open",
            "external":{"github-issues":{"issue":{"number":1,"url":"u","state":"open",
            "source":"balls","synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let t2 = task(
        r#"{"id":"bl-b","title":"b","status":"open",
            "external":{"github-issues":{"issue":{"number":2,"url":"u","state":"open",
            "source":"balls","synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let t3 = task(
        r#"{"id":"bl-c","title":"c","status":"open",
            "external":{"github-issues":{"issue":{"number":3,"url":"u","state":"open",
            "source":"balls","synced_at":"t","last_synced_status":"open"}}}}"#,
    );
    let t_no_num = task(r#"{"id":"bl-d","title":"d","status":"open"}"#);

    // GH knows only #2. Tasks 1 and 3 are externally deleted.
    // Task d has no stored number — skip.
    let known: std::collections::HashSet<u64> = [2].iter().copied().collect();
    let cfg = cfg_with(r#""on_external_delete":"deferred""#);

    // Cap = 1: only the first delete-candidate is emitted; the
    // overflow `break` fires after the second hit. Exercises line
    // out.len() >= max_emits.
    let out = sweep_deletes(&[t1.clone(), t2.clone(), t3.clone(), t_no_num.clone()], &known, &cfg, 1);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].task_id, "bl-a");

    // Cap = 10: both deletes emitted, in iteration order.
    let out = sweep_deletes(&[t1, t2, t3, t_no_num], &known, &cfg, 10);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].task_id, "bl-a");
    assert_eq!(out[1].task_id, "bl-c");
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
