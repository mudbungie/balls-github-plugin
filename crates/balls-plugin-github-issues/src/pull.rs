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
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-issues-test";

    fn cfg(label: Option<&str>) -> PluginConfig {
        let extra = label
            .map(|l| format!(r#","target_label":{:?}"#, l))
            .unwrap_or_default();
        serde_json::from_str(&format!(r#"{{"repo":"o/n"{}}}"#, extra)).unwrap()
    }

    fn issue(num: u64, title: &str, updated_at: &str, labels: &[&str]) -> GhIssue {
        GhIssue {
            number: num,
            title: title.into(),
            body: None,
            state: "open".into(),
            html_url: "u".into(),
            updated_at: updated_at.into(),
            labels: labels
                .iter()
                .map(|n| GhLabel { name: (*n).into() })
                .collect(),
        }
    }

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn matches_by_stored_number() {
        let i = issue(7, "anything", "2026-01-02T00:00:00Z", &[]);
        let t = task(
            r#"{"id":"bl-1","title":"t","status":"open",
                "external":{"github_issues":{"issue":{
                    "number":7,"url":"u","state":"open",
                    "source":"balls","synced_at":"2026-01-01T00:00:00+00:00",
                    "last_synced_status":"open"}}}}"#,
        );
        assert_eq!(
            classify(&i, &[t], &cfg(None)),
            Classification::KnownUpdate {
                task_id: "bl-1".into()
            }
        );
    }

    #[test]
    fn skips_when_synced_covers_update() {
        // synced_at strictly newer than updated_at -> skip
        let i = issue(7, "any", "2026-01-01T00:00:00Z", &[]);
        let t = task(
            r#"{"id":"bl-1","title":"t","status":"open",
                "external":{"github_issues":{"issue":{
                    "number":7,"url":"u","state":"open",
                    "source":"balls","synced_at":"2026-01-02T00:00:00+00:00",
                    "last_synced_status":"open"}}}}"#,
        );
        assert_eq!(
            classify(&i, &[t], &cfg(None)),
            Classification::Skip(SkipReason::LoopAvoidance)
        );
    }

    #[test]
    fn matches_by_title_tag_when_no_number() {
        let i = issue(99, "Title [bl-1a2b]", "2026-01-01T00:00:00Z", &[]);
        let t = task(r#"{"id":"bl-1a2b","title":"t","status":"open"}"#);
        assert_eq!(
            classify(&i, &[t], &cfg(None)),
            Classification::KnownUpdate {
                task_id: "bl-1a2b".into()
            }
        );
    }

    #[test]
    fn unmatched_issue_becomes_autocreate() {
        let i = issue(99, "External report", "2026-01-01T00:00:00Z", &[]);
        assert_eq!(classify(&i, &[], &cfg(None)), Classification::AutoCreate);
    }

    #[test]
    fn label_filter_skips_non_matching_issues() {
        let i = issue(99, "Without label", "2026-01-01T00:00:00Z", &[]);
        assert_eq!(
            classify(&i, &[], &cfg(Some("balls:track"))),
            Classification::Skip(SkipReason::LabelFilter)
        );

        let i2 = issue(
            99,
            "With label",
            "2026-01-01T00:00:00Z",
            &["other", "balls:track"],
        );
        assert_eq!(classify(&i2, &[], &cfg(Some("balls:track"))), Classification::AutoCreate);
    }

    #[test]
    fn malformed_title_tag_falls_through_to_autocreate() {
        // not hex
        assert!(extract_bl_id("nope [bl-xxxx]").is_none());
        // too short
        assert!(extract_bl_id("nope [bl-12]").is_none());
        // no brackets
        assert!(extract_bl_id("bl-1a2b").is_none());
        // valid (lowercase hex)
        assert_eq!(extract_bl_id("T [bl-1a2b]").as_deref(), Some("bl-1a2b"));
        // valid (longer id length)
        assert_eq!(
            extract_bl_id("T [bl-1a2b3c4d]").as_deref(),
            Some("bl-1a2b3c4d")
        );
    }

    #[test]
    fn unparseable_timestamps_do_not_skip() {
        // If either side is unparseable, classify must NOT skip on
        // loop-avoidance — the next layer needs to see the issue.
        assert!(!synced_covers_update("not-a-date", "2026-01-01T00:00:00Z"));
        assert!(!synced_covers_update("2026-01-01T00:00:00Z", "not-a-date"));
    }

    #[test]
    fn list_issues_round_trip() {
        let mut s = mockito::Server::new();
        s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
            .with_status(200)
            .with_body(
                r#"[{"number":1,"title":"a","state":"open","html_url":"u",
                     "updated_at":"2026-01-01T00:00:00Z","labels":[]}]"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let issues = list_issues(&c, "o", "n").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 1);
    }

    #[test]
    fn list_issues_propagates_api_error() {
        let mut s = mockito::Server::new();
        s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
            .with_status(503)
            .with_body("down")
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(list_issues(&c, "o", "n").is_err());
    }
}
