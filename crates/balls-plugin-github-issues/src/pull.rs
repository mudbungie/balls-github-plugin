//! Pull-side identity matching + loop avoidance. B4a establishes
//! the classification matrix; B4b/c/d each consume one
//! classification kind and emit the corresponding SyncReport entry.
//!
//! Why this lives in one module: doing identity once, here, is what
//! makes B4b/c/d clean leaves. If number-match / tag-match / loop-
//! avoidance lived inline in three call sites, drift would surface
//! as a duplicate-create or a ping-pong-status. The classifier is
//! the load-bearing seam.

use crate::config::PluginConfig;
use crate::issues_api::IssuesTaskExt;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::Task;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GhLabel {
    pub name: String,
}

/// The subset of a GH Issue the pull half consumes. Other API fields
/// are dropped (serde §13 forward-compat).
///
/// `#[allow(dead_code)]` on the fields that B4a populates but does
/// not yet read — `body`, `state`, `html_url` are consumed by
/// B4b's content-mirror and `created` emission. Keeping the fields
/// here means B4b/c add the consumers, not the fields.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GhIssue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    pub html_url: String,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<GhLabel>,
}

impl GhIssue {
    pub fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|l| l.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// `synced_at >= updated_at` — already saw this state.
    LoopAvoidance,
    /// Target-label filter is set and the issue lacks it.
    LabelFilter,
}

/// What the classifier says to do with a polled issue. B4a only
/// emits these variants; the actions are wired in B4b/c/d. The
/// `KnownDelete` variant is reserved for B4d (produced from a 404
/// fetch on a previously-mirrored issue, not from the list path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    Skip(SkipReason),
    KnownUpdate { task_id: String },
    AutoCreate,
    #[allow(dead_code)] // B4d emits this; the enum carries the future shape now.
    KnownDelete { task_id: String },
}

/// Extract `bl-xxxx` from a title like `"<anything> [bl-1a2b] <anything>"`.
/// The id is 4 hex chars by default (balls's `id_length` default); we
/// accept 4–32 hex to honor the `id_length` config knob without
/// requiring the plugin to read core's config.
fn extract_bl_id(title: &str) -> Option<String> {
    let start = title.find("[bl-")?;
    let rest = &title[start + 4..];
    let end = rest.find(']')?;
    let candidate = &rest[..end];
    if (4..=32).contains(&candidate.len()) && candidate.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("bl-{candidate}"))
    } else {
        None
    }
}

/// Compare RFC3339 timestamps. Loop avoidance returns true when the
/// task's `synced_at` is at least as recent as the issue's
/// `updated_at`, meaning we already saw (or wrote) this state.
fn synced_covers_update(synced_at: &str, updated_at: &str) -> bool {
    let s = chrono::DateTime::parse_from_rfc3339(synced_at);
    let u = chrono::DateTime::parse_from_rfc3339(updated_at);
    match (s, u) {
        (Ok(s), Ok(u)) => s >= u,
        // Unparseable on either side → don't skip; let the next layer
        // see the issue rather than silently swallow it.
        _ => false,
    }
}

pub fn classify(issue: &GhIssue, tasks: &[Task], config: &PluginConfig) -> Classification {
    if let Some(label) = &config.target_label {
        if !issue.has_label(label) {
            return Classification::Skip(SkipReason::LabelFilter);
        }
    }

    if let Some(task) = tasks
        .iter()
        .find(|t| t.issue_number() == Some(issue.number))
    {
        if let Some(synced) = task.synced_at() {
            if synced_covers_update(synced, &issue.updated_at) {
                return Classification::Skip(SkipReason::LoopAvoidance);
            }
        }
        return Classification::KnownUpdate {
            task_id: task.id.clone(),
        };
    }

    if let Some(id) = extract_bl_id(&issue.title) {
        if let Some(task) = tasks.iter().find(|t| t.id == id) {
            return Classification::KnownUpdate {
                task_id: task.id.clone(),
            };
        }
    }

    Classification::AutoCreate
}

/// `GET /repos/{o}/{n}/issues?state=all` — the bulk-listing entry
/// point B4a uses to feed classify. Pull pagination is intentionally
/// not handled in B4a (one page = whatever GH defaults to, 30); the
/// bl-4673 ingest backstops are the safety net, and B4c's
/// creates-flood tests stress them. Pagination is a future concern.
pub fn list_issues(client: &GithubClient, owner: &str, name: &str) -> Result<Vec<GhIssue>> {
    let url = format!("{}/repos/{}/{}/issues", client.api_base(), owner, name);
    let resp = GithubClient::check(
        client
            .auth(client.http().get(&url))
            .query(&[("state", "all")])
            .send()?,
    )?;
    Ok(resp.json()?)
}

#[cfg(test)]
#[path = "pull_tests.rs"]
mod tests;
