//! Push half: balls → GH issue create/update/close, idempotent.
//!
//! Decision matrix:
//!
//! | task.status     | stored number    | action            |
//! |-----------------|------------------|-------------------|
//! | open/in_progress| none             | POST (open issue) |
//! | any             | some, all        | noop (`{}`)       |
//! |                 |   last_synced_*  |                   |
//! |                 |   match          |                   |
//! | open/in_progress| some, any of     | PATCH (state=open)|
//! |                 |   status/title/  |                   |
//! |                 |   body moved     |                   |
//! | closed          | some, any of     | PATCH (close)     |
//! |                 |   status/title/  |                   |
//! |                 |   body moved     |                   |
//! | closed          | none             | noop (`{}`)       |
//!
//! The noop check compares status, the pushed title
//! (`<title> [<id>]`), and `body_hash(description)` against
//! `last_synced_*` written by the previous push. Any one differing
//! triggers a PATCH so balls-side title/body edits mirror to GH
//! (bl-73cd, symmetric counterpart to bl-4918's pull-side mirror).
//!
//! The `source:"balls"` marker on every emit is the loop-avoidance
//! hinge consumed by B4a: any GH issue whose latest state we
//! emitted ourselves is recognized as our own and skipped on pull.

use crate::config::PluginConfig;
use crate::issues_api::{create_issue, patch_issue, Issue, IssuesTaskExt};
use crate::pull_content::body_hash;
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
/// under `task.external.github-issues`).
pub fn push_task(client: &GithubClient, config: &PluginConfig, task: &Task) -> Result<Value> {
    let stored = task.issue_number();

    if task.status == "closed" && stored.is_none() {
        return Ok(json!({}));
    }

    let title = format!("{} [{}]", task.title, task.id);
    let body_h = body_hash(&task.description);

    if stored.is_some()
        && task.last_synced_status() == Some(task.status.as_str())
        && task.last_synced_title() == Some(title.as_str())
        && task.last_synced_body_hash() == Some(body_h.as_str())
    {
        return Ok(json!({}));
    }

    let (owner, name) = config
        .base
        .owner_name()
        .ok_or_else(|| PluginError::Config("repo is not owner/name".into()))?;
    let state = if task.status == "closed" { "closed" } else { "open" };

    let issue = match stored {
        Some(n) => patch_issue(client, owner, name, n, &title, &task.description, state)?,
        None => create_issue(client, owner, name, &title, &task.description)?,
    };
    Ok(issue_blob(&issue, &task.status, &title, &task.description))
}

fn issue_blob(
    issue: &Issue,
    last_synced_status: &str,
    last_synced_title: &str,
    body: &str,
) -> Value {
    json!({
        "issue": {
            "number": issue.number,
            "url": issue.html_url,
            "state": issue.state,
            "source": "balls",
            "synced_at": chrono::Utc::now().to_rfc3339(),
            "last_synced_status": last_synced_status,
            "last_synced_title": last_synced_title,
            "last_synced_body_hash": body_hash(body),
        }
    })
}

#[cfg(test)]
#[path = "push_tests.rs"]
mod tests;
