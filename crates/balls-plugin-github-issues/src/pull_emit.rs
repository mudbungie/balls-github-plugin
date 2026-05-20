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

use crate::config::{CloseMirror, PluginConfig};
use crate::pull::GhIssue;
use balls_github_shared::types::{SyncUpdate, Task};
use serde_json::Value;
use std::collections::BTreeMap;

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

#[cfg(test)]
#[path = "pull_emit_tests.rs"]
mod tests;
