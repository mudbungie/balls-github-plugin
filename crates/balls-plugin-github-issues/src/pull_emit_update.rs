//! `updated_from`: emit a single SyncUpdate combining close-mirror,
//! title-mirror, body-mirror, and a projection refresh for a
//! KnownUpdate-classified GH issue.
//!
//! Split out of `pull_emit.rs` so each emission shape (update vs
//! create vs delete) lives in its own ~200-line module under the
//! 300-line cap. The update path is the noisy one — three pieces of
//! merge logic plus the projection rebuild — so giving it a file of
//! its own keeps `pull_emit.rs` readable.
//!
//! The projection refresh on every emit is the bl-4918 hinge: without
//! it, the next sync poll sees `synced_at < updated_at` (we observed
//! the GH change but didn't record it), loop avoidance fails, and the
//! same SyncUpdate re-emits indefinitely. We rebuild the blob from
//! scratch (rather than patching the existing one) because core's
//! `apply_sync_report` *replaces* the participant key with whatever
//! the plugin sends — partial blobs would drop fields silently.

use crate::config::{CloseMirror, PluginConfig};
use crate::issues_api::IssuesTaskExt;
use crate::pull::GhIssue;
use crate::pull_content::{
    body_hash, decide_body, decide_title, pushed_title, strip_marker, FieldDecision,
};
use crate::pull_emit::{bound_body, MAX_BODY_BYTES};
use balls_github_shared::types::{SyncUpdate, Task};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn updated_from(issue: &GhIssue, task: &Task, config: &PluginConfig) -> Option<SyncUpdate> {
    let mut fields: BTreeMap<String, Value> = BTreeMap::new();
    let mut notes: Vec<String> = Vec::new();

    let close_emitted = apply_close_mirror(issue, task, config, &mut fields, &mut notes);
    apply_title_mirror(issue, task, &mut fields, &mut notes);
    apply_body_mirror(issue, task, &mut fields, &mut notes);

    if fields.is_empty() && notes.is_empty() {
        return None;
    }

    let new_status = if close_emitted {
        "closed"
    } else {
        task.status.as_str()
    };
    let new_title_field = fields
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(task.title.as_str());
    let new_description = fields
        .get("description")
        .and_then(Value::as_str)
        .map_or_else(|| task.description.clone(), str::to_string);

    let external = build_projection_blob(
        issue,
        task,
        new_status,
        &format!("{} [{}]", new_title_field, task.id),
        &body_hash(&new_description),
    );

    Some(SyncUpdate {
        task_id: task.id.clone(),
        fields,
        external,
        add_note: notes.join("\n"),
    })
}

fn apply_close_mirror(
    issue: &GhIssue,
    task: &Task,
    config: &PluginConfig,
    fields: &mut BTreeMap<String, Value>,
    notes: &mut Vec<String>,
) -> bool {
    if config.close_mirror == CloseMirror::Off {
        return false;
    }
    if issue.state != "closed" || task.status == "closed" {
        return false;
    }
    fields.insert("status".to_string(), Value::String("closed".to_string()));
    notes.push(format!(
        "GH issue #{} closed externally (close_mirror={})",
        issue.number,
        close_mirror_tag(config.close_mirror),
    ));
    true
}

fn apply_title_mirror(
    issue: &GhIssue,
    task: &Task,
    fields: &mut BTreeMap<String, Value>,
    notes: &mut Vec<String>,
) {
    let Some(last) = task.last_synced_title() else {
        // Legacy projection without last_synced_title: we can't tell
        // who moved, so we don't mirror. The next push will populate
        // the field and subsequent syncs will be decidable.
        return;
    };
    match decide_title(issue, task, last) {
        FieldDecision::Noop => {}
        FieldDecision::Mirror => {
            let new_balls_title = strip_marker(&issue.title, &task.id);
            fields.insert("title".to_string(), Value::String(new_balls_title));
            notes.push(format!(
                "GH issue #{} title mirrored from GH (was {:?})",
                issue.number, last,
            ));
        }
        FieldDecision::Conflict => {
            notes.push(format!(
                "GH issue #{} title conflict: GH={:?}, balls={:?} \
                 — leaving balls title (balls wins per merge contract)",
                issue.number,
                issue.title,
                pushed_title(task),
            ));
        }
    }
}

fn apply_body_mirror(
    issue: &GhIssue,
    task: &Task,
    fields: &mut BTreeMap<String, Value>,
    notes: &mut Vec<String>,
) {
    let Some(last) = task.last_synced_body_hash() else {
        return;
    };
    match decide_body(issue, task, last) {
        FieldDecision::Noop => {}
        FieldDecision::Mirror => {
            let gh_body = issue.body.as_deref().unwrap_or("");
            let (bounded, truncated) = bound_body(gh_body);
            let mut text = bounded;
            if truncated {
                text.push_str(&format!(
                    "\n\n[issues plugin: body truncated to \
                     {MAX_BODY_BYTES} bytes by ingest defense]"
                ));
            }
            fields.insert("description".to_string(), Value::String(text));
            notes.push(format!("GH issue #{} body mirrored from GH", issue.number));
        }
        FieldDecision::Conflict => {
            notes.push(format!(
                "GH issue #{} body conflict — leaving balls description \
                 (balls wins per merge contract)",
                issue.number
            ));
        }
    }
}

fn build_projection_blob(
    issue: &GhIssue,
    task: &Task,
    last_synced_status: &str,
    last_synced_title: &str,
    last_synced_body_hash: &str,
) -> serde_json::Map<String, Value> {
    // Preserve `source` if the existing projection has it (so an
    // auto-created task whose projection says `source: "github"`
    // doesn't suddenly flip to `"balls"` just because we observed an
    // update). Fall back to `"balls"` only when no prior source —
    // shouldn't happen on a KnownUpdate path, but the fallback keeps
    // the schema well-formed.
    let source = task
        .issue_blob()
        .and_then(|v| v.get("source"))
        .cloned()
        .unwrap_or_else(|| json!("balls"));

    let mut external = serde_json::Map::new();
    external.insert(
        "issue".to_string(),
        json!({
            "number": issue.number,
            "url": issue.html_url,
            "state": issue.state,
            "source": source,
            "synced_at": chrono::Utc::now().to_rfc3339(),
            "last_synced_status": last_synced_status,
            "last_synced_title": last_synced_title,
            "last_synced_body_hash": last_synced_body_hash,
        }),
    );
    external
}

pub(crate) fn close_mirror_tag(m: CloseMirror) -> &'static str {
    match m {
        CloseMirror::Authoritative => "authoritative",
        CloseMirror::BestEffort => "best_effort",
        CloseMirror::Off => "off",
    }
}

#[cfg(test)]
#[path = "pull_emit_update_tests.rs"]
mod tests;
