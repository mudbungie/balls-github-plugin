//! Tests for `pull_emit_update.rs` — close-mirror + content-mirror
//! emission plus the always-on projection refresh.

use super::*;
use crate::pull::GhLabel;
use crate::pull_content::body_hash;
use crate::pull_emit::MAX_BODY_BYTES;

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
        pull_request: None,
    }
}

fn issue_full(number: u64, title: &str, state: &str, body: Option<&str>) -> GhIssue {
    GhIssue {
        number,
        title: title.into(),
        body: body.map(str::to_string),
        state: state.into(),
        html_url: "u".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        labels: Vec::<GhLabel>::new(),
        pull_request: None,
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
    // Projection refresh fires on every emit.
    let blob = upd.external.get("issue").unwrap();
    assert_eq!(blob["last_synced_status"], "closed");
    assert_eq!(blob["state"], "closed");
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
fn skips_when_close_mirror_off_and_nothing_else_to_mirror() {
    let t = task(r#"{"id":"bl-3","title":"t","status":"open"}"#);
    let i = issue("closed", 11);
    assert!(updated_from(&i, &t, &cfg("off")).is_none());
}

#[test]
fn skips_when_gh_open_and_no_content_change() {
    let t = task(r#"{"id":"bl-4","title":"t","status":"open"}"#);
    let i = issue("open", 5);
    assert!(updated_from(&i, &t, &cfg("authoritative")).is_none());
}

#[test]
fn skips_when_balls_already_closed_and_no_content_change() {
    let t = task(r#"{"id":"bl-5","title":"t","status":"closed"}"#);
    let i = issue("closed", 13);
    assert!(updated_from(&i, &t, &cfg("authoritative")).is_none());
}

#[test]
fn close_mirror_tag_round_trips_all_variants() {
    assert_eq!(close_mirror_tag(CloseMirror::Authoritative), "authoritative");
    assert_eq!(close_mirror_tag(CloseMirror::BestEffort), "best_effort");
    assert_eq!(close_mirror_tag(CloseMirror::Off), "off");
}

fn task_with_projection(
    id: &str,
    status: &str,
    title: &str,
    description: &str,
    last_title: &str,
    last_body_hash: &str,
    source: &str,
) -> Task {
    let json = format!(
        r#"{{"id":"{id}","title":"{title}","status":"{status}","description":"{description}",
            "external":{{"github-issues":{{"issue":{{
                "number":7,"url":"u","state":"open","source":"{source}",
                "synced_at":"2026-01-01T00:00:00+00:00",
                "last_synced_status":"{status}",
                "last_synced_title":{last_title:?},
                "last_synced_body_hash":"{last_body_hash}"
            }}}}}}}}"#
    );
    serde_json::from_str(&json).unwrap()
}

#[test]
fn mirrors_gh_title_when_only_gh_moved() {
    let body = "same body";
    let hash = body_hash(body);
    let t = task_with_projection(
        "bl-1", "open", "Old title", body, "Old title [bl-1]", &hash, "balls",
    );
    let i = issue_full(7, "New title from GH [bl-1]", "open", Some(body));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    assert_eq!(
        upd.fields["title"],
        Value::String("New title from GH".into())
    );
    assert!(upd.add_note.contains("title mirrored"));
    // Projection's last_synced_title advances to the new GH-side
    // full title (which is what the bare title plus our marker will
    // be after this update lands).
    let blob = upd.external.get("issue").unwrap();
    assert_eq!(blob["last_synced_title"], "New title from GH [bl-1]");
}

#[test]
fn mirrors_gh_body_when_only_gh_moved() {
    let old_body = "old";
    let hash = body_hash(old_body);
    let t = task_with_projection(
        "bl-1", "open", "T", old_body, "T [bl-1]", &hash, "balls",
    );
    let i = issue_full(7, "T [bl-1]", "open", Some("new body from GH"));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    assert_eq!(
        upd.fields["description"],
        Value::String("new body from GH".into())
    );
    assert!(upd.add_note.contains("body mirrored"));
    let blob = upd.external.get("issue").unwrap();
    assert_eq!(
        blob["last_synced_body_hash"],
        body_hash("new body from GH")
    );
}

#[test]
fn title_conflict_when_both_moved_leaves_balls_with_conflict_note() {
    let body = "same";
    let hash = body_hash(body);
    let t = task_with_projection(
        "bl-1",
        "open",
        "Balls renamed",
        body,
        "Original [bl-1]",
        &hash,
        "balls",
    );
    let i = issue_full(7, "GH renamed [bl-1]", "open", Some(body));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    // No field update — balls's title is preserved.
    assert!(!upd.fields.contains_key("title"));
    assert!(upd.add_note.contains("title conflict"));
    assert!(upd.add_note.contains("GH renamed [bl-1]"));
    assert!(upd.add_note.contains("Balls renamed [bl-1]"));
    // Projection still refreshes, so the next sync doesn't re-emit
    // the same conflict note every poll.
    let blob = upd.external.get("issue").unwrap();
    assert_eq!(blob["last_synced_title"], "Balls renamed [bl-1]");
}

#[test]
fn body_conflict_when_both_moved_leaves_balls_with_conflict_note() {
    let last_hash = body_hash("original");
    let t = task_with_projection(
        "bl-1", "open", "T", "balls edit", "T [bl-1]", &last_hash, "balls",
    );
    let i = issue_full(7, "T [bl-1]", "open", Some("gh edit"));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    assert!(!upd.fields.contains_key("description"));
    assert!(upd.add_note.contains("body conflict"));
}

#[test]
fn pull_update_preserves_existing_source_in_projection() {
    // An auto-created task has source="github"; a later GH-side edit
    // mustn't flip it to "balls".
    let body = "x";
    let hash = body_hash(body);
    let t = task_with_projection(
        "bl-1", "open", "Old", body, "Old [bl-1]", &hash, "github",
    );
    let i = issue_full(7, "New [bl-1]", "open", Some(body));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    let blob = upd.external.get("issue").unwrap();
    assert_eq!(blob["source"], "github");
}

#[test]
fn second_sync_after_pull_update_is_noop() {
    // The acceptance criterion: pull-side projection refresh must
    // make the subsequent sync a noop. Simulate by feeding the
    // updated_from output back as the new projection.
    let body = "x";
    let hash = body_hash(body);
    let t = task_with_projection(
        "bl-1", "open", "Old", body, "Old [bl-1]", &hash, "balls",
    );
    let i = issue_full(7, "New [bl-1]", "open", Some(body));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();

    // Build the post-update task as core would after applying the
    // SyncUpdate: task.title becomes the new title, task.external is
    // replaced with the SyncUpdate's external map.
    let new_title = upd.fields["title"].as_str().unwrap().to_string();
    let updated_task: Task = serde_json::from_str(&format!(
        r#"{{"id":"bl-1","title":{new_title:?},"status":"open","description":"x",
            "external":{{"github-issues":{}}}}}"#,
        serde_json::to_string(&upd.external).unwrap(),
    ))
    .unwrap();

    assert!(updated_from(&i, &updated_task, &cfg("authoritative")).is_none());
}

#[test]
fn skips_when_projection_lacks_last_synced_title_and_no_status_change() {
    // Legacy task with only last_synced_status set. No title/body
    // mirror runs (no last_synced_* to compare against) and no status
    // transition — must return None.
    let t = task(
        r#"{"id":"bl-1","title":"X","status":"open","description":"b",
            "external":{"github-issues":{"issue":{
                "number":7,"url":"u","state":"open","source":"balls",
                "synced_at":"2026-01-01T00:00:00+00:00",
                "last_synced_status":"open"}}}}"#,
    );
    let i = issue_full(7, "X [bl-1]", "open", Some("b"));
    assert!(updated_from(&i, &t, &cfg("authoritative")).is_none());
}

#[test]
fn close_mirror_combines_with_content_mirror_in_one_update() {
    let body = "x";
    let hash = body_hash(body);
    let t = task_with_projection(
        "bl-1", "open", "Old", body, "Old [bl-1]", &hash, "balls",
    );
    let i = issue_full(7, "New [bl-1]", "closed", Some(body));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    assert_eq!(upd.fields["status"], Value::String("closed".into()));
    assert_eq!(upd.fields["title"], Value::String("New".into()));
    assert!(upd.add_note.contains("closed externally"));
    assert!(upd.add_note.contains("title mirrored"));
}

#[test]
fn mirrored_body_oversize_is_truncated_with_marker() {
    let big = "x".repeat(MAX_BODY_BYTES + 100);
    let hash = body_hash("old");
    let t = task_with_projection(
        "bl-1", "open", "T", "old", "T [bl-1]", &hash, "balls",
    );
    let i = issue_full(7, "T [bl-1]", "open", Some(&big));
    let upd = updated_from(&i, &t, &cfg("authoritative")).unwrap();
    let new_body = upd.fields["description"].as_str().unwrap();
    assert!(new_body.contains("truncated to"));
    assert!(new_body.len() < big.len());
}
