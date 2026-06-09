//! Push (balls → GitHub), the `*.post` direction (bl-613d).
//!
//! Wired on `create`/`update`/`close`/`drop` `post`. The sealed `bl-id` comes
//! from the post `metadata`; the ball's title and body are read from the STORE
//! checkout (`crate::store`) — the SAME source the pull side reads, so the two
//! directions never disagree on the body (bl-68db). The old code took the body
//! from the op's `command.body_change` *delta* (set only when this op passed
//! `--body`) while pull took it from the store, so a title-only edit, a
//! late-enabled mirror, or any op that didn't carry the body shipped an empty or
//! stale body — and the next `sync`, reading the real body from the store, saw
//! drift and re-PATCHed GitHub. `binding.store` is fast-forwarded to the sealed
//! commit before `post` runs (§8 seal), so the ball file is on disk to read.
//!
//! The reconciliation base (`crate::base`) is both the number cache (so a PATCH
//! needs no repo listing) and the idempotency oracle: an op that did not move
//! the title/body/state makes no API call, which is what closes the
//! pull→`bl update`→push loop (the base was advanced during the pull).
//!
//! When the import guard is set the handler suppresses itself entirely — a
//! pull-driven create/close must not echo back out to GitHub
//! (`crate::shellback`).

use std::path::Path;

use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use serde_json::json;

use crate::base::{Base, Snapshot};
use crate::content::body_hash;
use crate::issues_api;
use crate::marker;
use crate::store;
use crate::wire::Payload;

/// Reconcile the mapped GitHub issue for one sealed task op. `store_dir` is the
/// store checkout (`binding.store`), ff-merged to the seal before `post` runs,
/// so the ball's `tasks/<id>.md` is current. `base` is mutated and persisted to
/// `territory` on any change.
pub fn push(
    payload: &Payload,
    client: &GithubClient,
    owner: &str,
    name: &str,
    base: &mut Base,
    territory: &Path,
    store_dir: &Path,
) -> Result<()> {
    let id = payload
        .id()
        .ok_or_else(|| PluginError::Other("post payload carries no bl-id".into()))?;
    match payload.op.as_str() {
        "close" | "drop" => close_issue(client, owner, name, id, base, territory),
        "create" | "update" => upsert(client, owner, name, id, base, territory, store_dir),
        _ => Ok(()),
    }
}

/// Close the mapped issue (the retire ops). No-op if the task was never
/// mirrored, or its issue is already closed (idempotent).
fn close_issue(
    client: &GithubClient,
    owner: &str,
    name: &str,
    id: &str,
    base: &mut Base,
    territory: &Path,
) -> Result<()> {
    let Some(snap) = base.get(id).cloned() else {
        return Ok(());
    };
    if snap.state == "closed" {
        return Ok(());
    }
    issues_api::patch(client, owner, name, snap.number, &json!({ "state": "closed" }))?;
    base.set(id, Snapshot { state: "closed".into(), ..snap });
    base.save(territory)?;
    Ok(())
}

/// Create or update the mapped issue from the sealed ball. Title and body are
/// read from the store (the single source of truth both directions share): a new
/// task POSTs a fresh issue (outward mirror-on-create); an existing one PATCHes
/// only when the title, body, or state actually moved.
fn upsert(
    client: &GithubClient,
    owner: &str,
    name: &str,
    id: &str,
    base: &mut Base,
    territory: &Path,
    store_dir: &Path,
) -> Result<()> {
    let ball = store::read_ball(store_dir, id)?.ok_or_else(|| {
        PluginError::Other(format!("create/update post: ball {id} absent from the store"))
    })?;
    let bare = ball.title;
    let body = ball.body;
    let marked = marker::append(&bare, id);

    if let Some(snap) = base.get(id).cloned() {
        let title_moved = bare != snap.title;
        let body_moved = body_hash(&body) != snap.body_hash;
        let reopen = snap.state != "open";
        if !(title_moved || body_moved || reopen) {
            return Ok(()); // nothing moved — loop-avoidance no-op
        }
        let fields = json!({ "title": marked, "state": "open", "body": &body });
        issues_api::patch(client, owner, name, snap.number, &fields)?;
        base.set(id, Snapshot { number: snap.number, title: bare, body_hash: body_hash(&body), state: "open".into() });
    } else {
        let issue = issues_api::create_issue(client, owner, name, &marked, &body)?;
        base.set(id, Snapshot { number: issue.number, title: bare, body_hash: body_hash(&body), state: issue.state });
    }
    base.save(territory)?;
    Ok(())
}

#[cfg(test)]
#[path = "push_tests.rs"]
mod tests;
