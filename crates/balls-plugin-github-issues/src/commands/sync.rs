//! Sync half (GH -> balls via SyncReport).
//!
//! Wiring:
//! 1. List GH issues for the configured repo.
//! 2. Classify each issue (`pull::classify`) and emit per-kind
//!    entries: KnownUpdate → updated, AutoCreate → created.
//! 3. After the list-walk, scan the input tasks for any stored
//!    issue numbers that did NOT appear in the GH list → those are
//!    "deleted from GH"; emit per `on_external_delete` policy.
//!
//! `list_issues` paginates to completion (bl-bb66), so the
//! `known_numbers` fed to the delete-sweep is the COMPLETE issue set;
//! an off-page-1 issue no longer reads as deleted. MAX_DELETES_PER_SYNC
//! remains as a defense-in-depth bound on a genuine mass-delete, not a
//! patch over a truncated listing.

use crate::config::PluginConfig;
use crate::pull::{classify, list_issues, Classification};
use crate::pull_emit::{
    created_from, sweep_deletes, updated_from, MAX_CREATES_PER_SYNC, MAX_DELETES_PER_SYNC,
};
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::{SyncReport, Task};
use std::collections::HashSet;
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
    let known_numbers: HashSet<u64> = issues.iter().map(|i| i.number).collect();
    let mut report = SyncReport::default();

    for issue in &issues {
        match classify(issue, &tasks, &config) {
            Classification::KnownUpdate { task_id } => {
                let task = tasks
                    .iter()
                    .find(|t| t.id == task_id)
                    .expect("KnownUpdate carries a task_id present in `tasks`");
                if let Some(upd) = updated_from(issue, task, &config) {
                    report.updated.push(upd);
                }
            }
            Classification::AutoCreate => {
                if report.created.len() < MAX_CREATES_PER_SYNC {
                    report.created.push(created_from(issue));
                }
            }
            Classification::KnownDelete { .. } | Classification::Skip(_) => {}
        }
    }

    // B4d: balls tasks whose stored issue number is not in the
    // (fully-paginated) listing are externally deleted. The cap is
    // enforced in sweep_deletes so the overflow branch is testable
    // with a small cap rather than 500+ fixture tasks.
    report
        .updated
        .extend(sweep_deletes(&tasks, &known_numbers, &config, MAX_DELETES_PER_SYNC));

    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
