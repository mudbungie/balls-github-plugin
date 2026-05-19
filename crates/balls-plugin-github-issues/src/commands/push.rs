//! Push half: balls → GH issue create/update/close, idempotent.
//!
//! Decision matrix:
//!
//! | task.status     | stored number    | action            |
//! |-----------------|------------------|-------------------|
//! | open/in_progress| none             | POST (open issue) |
//! | any             | some, status=    | noop (`{}`)       |
//! |                 |   last_synced    |                   |
//! | open/in_progress| some, changed    | PATCH (state=open)|
//! | closed          | some, changed    | PATCH (close)     |
//! | closed          | none             | noop (`{}`)       |
//!
//! The `source:"balls"` marker on every emit is the loop-avoidance
//! hinge consumed by B4a: any GH issue whose latest state we
//! emitted ourselves is recognized as our own and skipped on pull.

use crate::config::PluginConfig;
use crate::issues_api::{create_issue, patch_issue, Issue, IssuesTaskExt};
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::Task;
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

pub fn run(task_id: &str, config_path: &Path, auth_dir: &Path) -> Result<()> {
    let config = PluginConfig::load(config_path)?;
    let token = auth::load_token(auth_dir)?;
    let client = GithubClient::new(config.api_base(), &token, USER_AGENT);

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let task: Task = serde_json::from_str(&buf)?;
    if task.id != task_id {
        return Err(PluginError::Other(format!(
            "--task {} does not match stdin task id {}",
            task_id, task.id
        )));
    }

    let resp = push_task(&client, &config, &task)?;
    println!("{}", resp);
    Ok(())
}

/// Returns the JSON to print on stdout: either `{}` (noop) or
/// `{"issue": {...}}` (the per-participant external blob core stores
/// under `task.external.github_issues`).
pub fn push_task(client: &GithubClient, config: &PluginConfig, task: &Task) -> Result<Value> {
    let stored = task.issue_number();
    let last = task.last_synced_status();

    if task.status == "closed" && stored.is_none() {
        return Ok(json!({}));
    }
    if stored.is_some() && last == Some(task.status.as_str()) {
        return Ok(json!({}));
    }

    let (owner, name) = config
        .base
        .owner_name()
        .ok_or_else(|| PluginError::Config("repo is not owner/name".into()))?;
    let title = format!("{} [{}]", task.title, task.id);
    let state = if task.status == "closed" { "closed" } else { "open" };

    let issue = match stored {
        Some(n) => patch_issue(client, owner, name, n, &title, &task.description, state)?,
        None => create_issue(client, owner, name, &title, &task.description)?,
    };
    Ok(issue_blob(&issue, &task.status))
}

fn issue_blob(issue: &Issue, last_synced_status: &str) -> Value {
    json!({
        "issue": {
            "number": issue.number,
            "url": issue.html_url,
            "state": issue.state,
            "source": "balls",
            "synced_at": chrono::Utc::now().to_rfc3339(),
            "last_synced_status": last_synced_status,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-issues-test";

    fn cfg(api: &str) -> PluginConfig {
        serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":{:?}}}"#, api)).unwrap()
    }

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn noop_when_closed_without_stored_number() {
        let c = GithubClient::new("http://x", "t", UA);
        let v = push_task(
            &c,
            &cfg("http://x"),
            &task(r#"{"id":"bl-1","title":"t","status":"closed"}"#),
        )
        .unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn noop_when_status_unchanged_since_last_sync() {
        let c = GithubClient::new("http://x", "t", UA);
        let v = push_task(
            &c,
            &cfg("http://x"),
            &task(
                r#"{"id":"bl-2","title":"t","status":"open",
                    "external":{"github_issues":{"issue":{
                        "number":3,"url":"u","state":"open",
                        "source":"balls","synced_at":"t",
                        "last_synced_status":"open"}}}}"#,
            ),
        )
        .unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn rejects_bad_repo() {
        let c = GithubClient::new("http://x", "t", UA);
        let conf: PluginConfig = serde_json::from_str(r#"{"repo":"noslash"}"#).unwrap();
        assert!(push_task(
            &c,
            &conf,
            &task(r#"{"id":"bl-1","title":"t","status":"open"}"#),
        )
        .is_err());
    }

    #[test]
    fn creates_issue_when_no_stored_number() {
        let mut s = mockito::Server::new();
        s.mock("POST", "/repos/o/n/issues")
            .with_status(201)
            .with_body(r#"{"number":7,"html_url":"https://gh/i/7","state":"open"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let v = push_task(
            &c,
            &cfg(&s.url()),
            &task(r#"{"id":"bl-3","title":"Do it","status":"open","description":"body"}"#),
        )
        .unwrap();
        let issue = &v["issue"];
        assert_eq!(issue["number"], 7);
        assert_eq!(issue["source"], "balls");
        assert_eq!(issue["last_synced_status"], "open");
    }

    #[test]
    fn patches_existing_issue_on_status_change_close() {
        let mut s = mockito::Server::new();
        s.mock("PATCH", "/repos/o/n/issues/4")
            .with_status(200)
            .with_body(r#"{"number":4,"html_url":"u","state":"closed"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let v = push_task(
            &c,
            &cfg(&s.url()),
            &task(
                r#"{"id":"bl-4","title":"t","status":"closed",
                    "external":{"github_issues":{"issue":{
                        "number":4,"url":"u","state":"open",
                        "source":"balls","synced_at":"t",
                        "last_synced_status":"open"}}}}"#,
            ),
        )
        .unwrap();
        let issue = &v["issue"];
        assert_eq!(issue["state"], "closed");
        assert_eq!(issue["last_synced_status"], "closed");
    }

    #[test]
    fn patches_existing_issue_on_status_change_reopen() {
        // open -> in_progress is a status change even though both
        // map to GH state=open; we still PATCH so the title/body
        // mirror moves and the last_synced_status updates.
        let mut s = mockito::Server::new();
        s.mock("PATCH", "/repos/o/n/issues/5")
            .with_status(200)
            .with_body(r#"{"number":5,"html_url":"u","state":"open"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let v = push_task(
            &c,
            &cfg(&s.url()),
            &task(
                r#"{"id":"bl-5","title":"t","status":"in_progress",
                    "external":{"github_issues":{"issue":{
                        "number":5,"url":"u","state":"open",
                        "source":"balls","synced_at":"t",
                        "last_synced_status":"open"}}}}"#,
            ),
        )
        .unwrap();
        assert_eq!(v["issue"]["last_synced_status"], "in_progress");
    }
}
