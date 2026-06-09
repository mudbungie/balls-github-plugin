//! Pull (GitHub → balls), the `sync` direction (bl-613d).
//!
//! Wired on `sync` (cwd = the live store checkout, §13). There is no return
//! channel: instead of reporting changes for core to apply, the handler drives
//! `bl` directly (`crate::shellback`). Per the locked authority model — balls
//! owns content, GitHub owns only the close transition inward — the inward
//! actions are exactly:
//! - **auto-create**: an in-scope GitHub issue with no `[bl-xxxx]` marker and no
//!   base link becomes a `bl create`; we then stamp the marker back onto it.
//! - **close mirror**: a closed GitHub issue closes the live task (`bl close`).
//! - **external-delete sweep**: a base-linked issue gone from the repo defers or
//!   closes the task per `on_external_delete`.
//! - **content re-assert**: when a still-open issue drifts from its ball, balls
//!   wins — the ball's title/body is re-PATCHed OUT to GitHub.
//!
//! Reads of the store are direct (the title/body for comparison, the live-id
//! set); every WRITE goes through a shelled verb so the lifecycle hooks run.

use std::collections::HashSet;
use std::path::Path;

use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use serde_json::json;

use crate::base::{Base, Snapshot};
use crate::config::{OnExternalDelete, PluginConfig};
use crate::content::{body_hash, differs, mirror_close};
use crate::issues_api::{self, GhIssue};
use crate::shellback::Bl;
use crate::{marker, store};

/// GitHub bodies can be huge; cap what we ingest into a ball. The cap is applied
/// consistently (the ball receives the truncated body, and comparison truncates
/// GitHub's body the same way) so a long issue never reads as perpetual drift.
const MAX_BODY_BYTES: usize = 65_536;
/// Cap labels mirrored onto an auto-created task.
const MAX_LABELS: usize = 100;
/// Bound auto-creates per sync — a runaway backstop (logged, never silent).
const MAX_AUTOCREATES: usize = 500;

/// Run the pull reconcile. `store_dir` is the sync cwd; `territory` holds the base.
#[allow(clippy::too_many_arguments)]
pub fn pull(
    client: &GithubClient,
    owner: &str,
    name: &str,
    cfg: &PluginConfig,
    base: &mut Base,
    store_dir: &Path,
    territory: &Path,
    bl: &Bl,
) -> Result<()> {
    let issues = issues_api::list_issues(client, owner, name)?;
    let known: HashSet<u64> = issues.iter().map(|i| i.number).collect();
    let mut created = 0usize;
    for issue in &issues {
        if cfg.target_label.as_ref().is_some_and(|l| !issue.has_label(l)) {
            continue;
        }
        reconcile(issue, cfg, base, store_dir, bl, client, owner, name, &mut created)?;
    }
    sweep(base, &known, cfg, store_dir, bl)?;
    base.save(territory)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn reconcile(
    issue: &GhIssue,
    cfg: &PluginConfig,
    base: &mut Base,
    store_dir: &Path,
    bl: &Bl,
    client: &GithubClient,
    owner: &str,
    name: &str,
    created: &mut usize,
) -> Result<()> {
    let (bare_ref, marker_id) = marker::strip(&issue.title);
    let bare = bare_ref.to_string();
    let linked = marker_id
        .map(str::to_string)
        .or_else(|| base.id_for_number(issue.number).map(str::to_string));
    match linked {
        Some(id) => linked_issue(&id, &bare, issue, cfg, base, store_dir, bl, client, owner, name),
        None => autocreate(&bare, issue, cfg, base, bl, client, owner, name, created),
    }
}

#[allow(clippy::too_many_arguments)]
fn linked_issue(
    id: &str,
    bare: &str,
    issue: &GhIssue,
    cfg: &PluginConfig,
    base: &mut Base,
    store_dir: &Path,
    bl: &Bl,
    client: &GithubClient,
    owner: &str,
    name: &str,
) -> Result<()> {
    let Some(ball) = store::read_ball(store_dir, id)? else {
        // Task is gone; catch up an outward close the close.post may have missed,
        // then forget the stale link.
        if !issue.is_closed() {
            issues_api::patch(client, owner, name, issue.number, &json!({ "state": "closed" }))?;
        }
        base.remove(id);
        return Ok(());
    };
    // The one inward content exception: a GitHub close closes the task.
    if mirror_close(issue.is_closed(), true, cfg.close_mirror) {
        bl.close(id)?;
        base.remove(id);
        return Ok(());
    }
    // Content: balls wins. If the (effective) GitHub view drifts, re-assert.
    let gh_body = truncate_body(issue.body_str());
    if differs(bare, &gh_body, &ball.title, &ball.body) {
        let marked = marker::append(&ball.title, id);
        issues_api::patch(
            client,
            owner,
            name,
            issue.number,
            &json!({ "title": marked, "body": ball.body }),
        )?;
    }
    base.set(
        id,
        Snapshot {
            number: issue.number,
            title: ball.title,
            body_hash: body_hash(&ball.body),
            state: issue.state.clone(),
        },
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn autocreate(
    bare: &str,
    issue: &GhIssue,
    cfg: &PluginConfig,
    base: &mut Base,
    bl: &Bl,
    client: &GithubClient,
    owner: &str,
    name: &str,
    created: &mut usize,
) -> Result<()> {
    if issue.is_closed() {
        return Ok(()); // don't import an already-closed external issue as new work
    }
    if *created >= MAX_AUTOCREATES {
        eprintln!(
            "github-issues: auto-create cap {MAX_AUTOCREATES} hit; issue #{} deferred to next sync",
            issue.number
        );
        return Ok(());
    }
    let body = truncate_body(issue.body_str());
    let labels = labels_of(issue, cfg);
    let id = bl.create(bare, &body, &labels)?;
    // Stamp the marker back so the next sync recognises the link (SSOT join).
    let marked = marker::append(bare, &id);
    issues_api::patch(client, owner, name, issue.number, &json!({ "title": marked }))?;
    base.set(
        &id,
        Snapshot {
            number: issue.number,
            title: bare.to_string(),
            body_hash: body_hash(&body),
            state: issue.state.clone(),
        },
    );
    *created += 1;
    Ok(())
}

/// External-delete sweep: a base-linked issue absent from `known` vanished from
/// the repo. Defer/close the live task per policy; forget a stale link.
fn sweep(
    base: &mut Base,
    known: &HashSet<u64>,
    cfg: &PluginConfig,
    store_dir: &Path,
    bl: &Bl,
) -> Result<()> {
    let vanished: Vec<String> = base
        .entries
        .iter()
        .filter(|(_, s)| !known.contains(&s.number))
        .map(|(id, _)| id.clone())
        .collect();
    for id in vanished {
        if !store::is_live(store_dir, &id) {
            base.remove(&id);
            continue;
        }
        match cfg.on_external_delete {
            OnExternalDelete::Deferred => bl.add_tag(&id, "deferred")?,
            OnExternalDelete::Closed => {
                bl.close(&id)?;
                base.remove(&id);
            }
            OnExternalDelete::Noop => {}
        }
    }
    Ok(())
}

/// Truncate an ingested body to [`MAX_BODY_BYTES`] on a char boundary, marking
/// the cut. Applied symmetrically so it never reads as drift.
fn truncate_body(body: &str) -> String {
    if body.len() <= MAX_BODY_BYTES {
        return body.to_string();
    }
    let mut end = MAX_BODY_BYTES;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\n[github-issues: body truncated by ingest defense]", &body[..end])
}

/// Label names mirrored onto an auto-created task: the target label is dropped
/// (it is a tracking marker, not a semantic tag), and the list is capped.
fn labels_of(issue: &GhIssue, cfg: &PluginConfig) -> Vec<String> {
    issue
        .labels
        .iter()
        .map(|l| l.name.clone())
        .filter(|n| cfg.target_label.as_deref() != Some(n))
        .take(MAX_LABELS)
        .collect()
}

#[cfg(test)]
#[path = "pull_tests.rs"]
mod tests;
