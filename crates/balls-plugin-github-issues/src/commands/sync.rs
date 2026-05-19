//! Sync half (GH -> balls via SyncReport).
//!
//! B4a wires the end-to-end pull skeleton: load config + token, list
//! issues from GH, parse the task list from stdin, classify each
//! issue (loop-avoidance / known-update / auto-create / skipped per
//! label filter). No SyncReport entries are emitted yet — B4b/c/d
//! consume KnownUpdate / AutoCreate / KnownDelete respectively.
//!
//! Emitting an empty report (`{}`) is a valid no-op per balls's
//! plugin protocol; the classification calls run but their results
//! are dropped here. The unit tests on `pull::classify` are what
//! prove the matching logic; B6's integration test wires the full
//! lifecycle once B4b/c/d land.

use crate::config::PluginConfig;
use crate::pull::{classify, list_issues};
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::{SyncReport, Task};
use std::io::Read;
use std::path::Path;

pub fn run(_filter: Option<&str>, config_path: &Path, auth_dir: &Path) -> Result<()> {
    let config = PluginConfig::load(config_path)?;
    let token = auth::load_token(auth_dir)?;
    let client = GithubClient::new(config.api_base(), &token, USER_AGENT);
    let (owner, name) = config
        .base
        .owner_name()
        .ok_or_else(|| PluginError::Config("repo is not owner/name".into()))?;

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let tasks: Vec<Task> = if buf.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&buf)?
    };

    // B4a only classifies — no entries emitted. The classification
    // touches every reachable branch; B4b/c/d add the per-kind
    // emission and the tests against the classified shape.
    let issues = list_issues(&client, owner, name)?;
    for issue in &issues {
        let _ = classify(issue, &tasks, &config);
    }

    println!("{}", serde_json::to_string(&SyncReport::default())?);
    Ok(())
}
