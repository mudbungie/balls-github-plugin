//! Sync half (GH -> balls via SyncReport).
//!
//! Wiring: list GH issues, classify each (`pull::classify`), and for
//! each KnownUpdate ask `pull_emit::updated_from` whether to emit a
//! SyncReport `updated` entry. B4b lands the close-mirror; B4c
//! (auto-create from AutoCreate) and B4d (delete handling from
//! KnownDelete) add the remaining two entry kinds.

use crate::config::PluginConfig;
use crate::pull::{classify, list_issues, Classification};
use crate::pull_emit::updated_from;
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

    let issues = list_issues(&client, owner, name)?;
    let mut report = SyncReport::default();
    for issue in &issues {
        if let Classification::KnownUpdate { task_id } = classify(issue, &tasks, &config) {
            let task = tasks
                .iter()
                .find(|t| t.id == task_id)
                .expect("KnownUpdate carries a task_id present in `tasks`");
            if let Some(upd) = updated_from(issue, task, &config) {
                report.updated.push(upd);
            }
        }
        // AutoCreate / KnownDelete / Skip are unhandled here;
        // B4c and B4d add the create and delete emission paths.
    }

    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
