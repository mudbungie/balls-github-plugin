//! Emit SyncReport entries from classified GH issues.
//!
//! B4b ships the close-mirror half: GH-issue closed externally
//! propagates to the mapped balls task as `status="closed"`, gated
//! by the operator's `close_mirror` policy. The richer title/body
//! content mirror + conflict-via-SyncReport machinery is scoped
//! to a follow-up ball — those need balls-core sync-report
//! semantics work (writing into `external.*` from a SyncUpdate) and
//! a hash-bounded `last_synced_*` projection that B3's push doesn't
//! yet populate. Close-mirror sidesteps both: GH and balls only ever
//! diverge on `status`, status is a top-level Task field core
//! already accepts in SyncReport.updated.fields, and the absence of
//! `last_synced_status` tracking on the pull side is harmless
//! because a second sync sees task.status == gh.state and skips.

use crate::config::{CloseMirror, OnExternalDelete, PluginConfig};
use crate::issues_api::IssuesTaskExt;
use crate::pull::GhIssue;
use balls_github_shared::types::{SyncCreate, SyncUpdate, Task};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Per-issue body size cap on auto-create. The bl-4673 ingest
/// backstops in balls core handle anything pathological; this is a
/// plugin-side conservative truncation so the operator sees an
/// explicit truncation marker rather than a silent core-side cut.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// Per-issue label cap on auto-create. GH allows up to 100 labels
/// per issue today; a pathological repo with thousands would still
/// be bounded here. The cap matches the documented ingest defense.
pub const MAX_LABELS: usize = 100;

/// Maximum `created` entries one sync invocation can emit. Beyond
/// this, remaining unmatched issues are paged to the next sync (the
/// classifier still flags them as AutoCreate on the next poll). This
/// is the creates-flood guard from B4c's acceptance criteria.
pub const MAX_CREATES_PER_SYNC: usize = 500;

/// Maximum delete-policy emissions one sync invocation can produce.
/// Mirrors MAX_CREATES_PER_SYNC for the inverse direction — bounds
/// the mass-delete-shaped attack from B4d's acceptance.
pub const MAX_DELETES_PER_SYNC: usize = 500;

/// Given a classified KnownUpdate (the matched GH issue + its
/// mapped balls task + the plugin config), return the SyncUpdate
/// to emit — or None if nothing should change.
pub fn updated_from(issue: &GhIssue, task: &Task, config: &PluginConfig) -> Option<SyncUpdate> {
    if config.close_mirror == CloseMirror::Off {
        return None;
    }
    if issue.state != "closed" {
        return None;
    }
    if task.status == "closed" {
        return None;
    }

    let mut fields = BTreeMap::new();
    fields.insert(
        "status".to_string(),
        Value::String("closed".to_string()),
    );
    Some(SyncUpdate {
        task_id: task.id.clone(),
        fields,
        add_note: format!(
            "GH issue #{} closed externally (close_mirror={})",
            issue.number,
            close_mirror_tag(config.close_mirror),
        ),
    })
}

pub(crate) fn close_mirror_tag(m: CloseMirror) -> &'static str {
    match m {
        CloseMirror::Authoritative => "authoritative",
        CloseMirror::BestEffort => "best_effort",
        CloseMirror::Off => "off",
    }
}

/// Build the SyncCreate from a GH issue classified AutoCreate.
/// Applies the bl-4673 plugin-side defenses: bounded body, bounded
/// label count. The truncation/cap conditions are appended as a
/// suffix to the description so the operator sees what happened.
pub fn created_from(issue: &GhIssue) -> SyncCreate {
    let (description, body_truncated) = bound_body(issue.body.as_deref().unwrap_or(""));
    let (tags, label_truncated) = bound_labels(&issue.labels);

    let mut notes = Vec::new();
    if body_truncated {
        notes.push(format!(
            "[issues plugin: body truncated to {} bytes by ingest defense]",
            MAX_BODY_BYTES
        ));
    }
    if label_truncated {
        notes.push(format!(
            "[issues plugin: label set truncated to first {} by ingest defense]",
            MAX_LABELS
        ));
    }
    let description = if notes.is_empty() {
        description
    } else {
        format!("{description}\n\n{}", notes.join("\n"))
    };

    // Core inserts SyncCreate.external verbatim under the
    // participant name (see plugin/types.rs PushResponse docs). Emit
    // the inner blob directly — wrapping under "github_issues" here
    // is what made every sync poll re-create the same task
    // (bl-a2ea defect 1).
    let mut external = serde_json::Map::new();
    external.insert(
        "issue".to_string(),
        json!({
            "number": issue.number,
            "url": issue.html_url,
            "state": issue.state,
            "source": "github",
            "synced_at": chrono::Utc::now().to_rfc3339(),
            "last_synced_status": "open",
        }),
    );

    SyncCreate {
        title: issue.title.clone(),
        task_type: "task".to_string(),
        priority: 3,
        status: "open".to_string(),
        description,
        tags,
        external,
    }
}

fn bound_body(body: &str) -> (String, bool) {
    if body.len() <= MAX_BODY_BYTES {
        (body.to_string(), false)
    } else {
        // Truncate at a char boundary so we don't slice mid-UTF-8.
        let mut idx = MAX_BODY_BYTES;
        while !body.is_char_boundary(idx) && idx > 0 {
            idx -= 1;
        }
        (body[..idx].to_string(), true)
    }
}

/// Sweep tasks for stored issue numbers no longer present in
/// `known_numbers` and emit per `on_external_delete` policy.
/// Capped by `max_emits` so a mass-delete (or a pagination-induced
/// false-positive) is bounded — the runtime call site uses
/// `MAX_DELETES_PER_SYNC`; tests pass a smaller cap to exercise the
/// overflow branch without needing 500+ fixture tasks.
pub fn sweep_deletes(
    tasks: &[Task],
    known_numbers: &std::collections::HashSet<u64>,
    config: &PluginConfig,
    max_emits: usize,
) -> Vec<SyncUpdate> {
    let mut out = Vec::new();
    for task in tasks {
        if out.len() >= max_emits {
            break;
        }
        if let Some(num) = task.issue_number() {
            if !known_numbers.contains(&num) {
                if let Some(upd) = deleted_from(task, config) {
                    out.push(upd);
                }
            }
        }
    }
    out
}

/// A mirrored balls task whose GH issue number is no longer in the
/// listed issues — treated as "deleted from GH". Returns the
/// SyncUpdate to flip the balls task's status per
/// `on_external_delete`, or None when the policy is Noop or the
/// task is already at the target status (idempotent).
pub fn deleted_from(task: &Task, config: &PluginConfig) -> Option<SyncUpdate> {
    let target_status = match config.on_external_delete {
        OnExternalDelete::Noop => return None,
        OnExternalDelete::Deferred => "deferred",
        OnExternalDelete::Closed => "closed",
    };
    if task.status == target_status {
        return None;
    }
    let number = task.issue_number()?;

    let mut fields = BTreeMap::new();
    fields.insert(
        "status".to_string(),
        Value::String(target_status.to_string()),
    );
    Some(SyncUpdate {
        task_id: task.id.clone(),
        fields,
        add_note: format!(
            "GH issue #{} no longer found in repo (on_external_delete={})",
            number,
            on_external_delete_tag(config.on_external_delete),
        ),
    })
}

pub(crate) fn on_external_delete_tag(p: OnExternalDelete) -> &'static str {
    match p {
        OnExternalDelete::Deferred => "deferred",
        OnExternalDelete::Closed => "closed",
        OnExternalDelete::Noop => "noop",
    }
}

fn bound_labels(labels: &[crate::pull::GhLabel]) -> (Vec<String>, bool) {
    let total = labels.len();
    let kept: Vec<String> = labels
        .iter()
        .take(MAX_LABELS)
        .map(|l| l.name.clone())
        .collect();
    (kept, total > MAX_LABELS)
}

#[cfg(test)]
#[path = "pull_emit_tests.rs"]
mod tests;
