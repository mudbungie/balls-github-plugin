//! The forge delivery variant's POLICY — the §6/§7 hook-dispatch matrix.
//!
//! This is the FORGE variant of the §11 delivery plugin: it is wired ALONGSIDE
//! the worktree-owning `bl-delivery` (which still materializes/tears down the
//! `work/<id>` code worktree), and differs only in the delivery hooks it takes
//! over — exactly what §11 means by "the two variants differ only in what's
//! wired into the delivery hooks":
//!
//! - **`claim.post`** — open an approval **gate child** (a normal close-blocker
//!   on the parent, §10 — NOT a special mechanism), remembering its id in the
//!   plugin's own territory (§1/§7 give it no return channel to store the link
//!   on the ball).
//! - **`close.pre`** — capture pending worktree work, then EITHER push
//!   `work/<id>` + open/update the PR (the forge produces the squash on merge,
//!   so we never squash locally), OR — when there is nothing to review (the §11
//!   empty deliverable) — auto-resolve the gate by closing it.
//! - **`sync.post`** — close the gate child of every still-open task whose PR
//!   has merged, unblocking the parent's next `bl close`.
//! - **`unclaim.post`** — tear the PR/branch down (abandonment is `unclaim`
//!   then `close`, §11/§15 bl-65e0 — the `drop` verb is gone). The gate child
//!   STAYS: the parent is still open and the gate is its §10 close-blocker —
//!   resolving it here would re-open the gate bypass deleting `drop` closed.
//!   The gate resolves later: empty `close.pre` auto-resolves it, a re-claim
//!   reuses it (PR → merge → `sync`), or an approver closes it by hand.
//!
//! **Rollback (§14):** `rollback claim.post` closes the just-opened gate child;
//! `rollback close.pre` is a NO-OP — a pushed branch + open PR is the correct
//! in-review state, never undone (abandon is `bl unclaim` then `bl close`).
//!
//! The matrix is pure: every side effect goes through the [`Forge`] seam, so it
//! is unit-tested against a fake without a repo, a network, or a real `bl`.

use balls_github_shared::error::Result;

/// The protocol self-description (`<bin> protocol`, §6): this plugin speaks
/// protocol 1 and handles the delivery ops whose hooks it wires into. balls
/// reads it at install time, validates the wiring against it, and never persists
/// it.
pub const PROTOCOL_JSON: &str = r#"{"protocol":[1],"ops":["claim","close","unclaim","sync"]}"#;

/// The side-effecting acts the forge hooks need, behind a seam so [`dispatch`]
/// is testable without a real repo / network / `bl`. The real impl is
/// [`crate::project::Project`].
pub trait Forge {
    /// `claim.post`: `bl create` the approval gate child of `parent`, returning
    /// its minted id.
    fn create_gate(&self, parent: &str, title: &str) -> Result<String>;
    /// Persist the `parent → gate` link in the plugin's territory (§1).
    fn remember_gate(&self, parent: &str, gate: &str) -> Result<()>;
    /// Read back the gate child id for `parent` (`None` if none recorded).
    fn recall_gate(&self, parent: &str) -> Result<Option<String>>;
    /// Forget the `parent → gate` link (the gate is resolved or abandoned).
    fn forget_gate(&self, parent: &str) -> Result<()>;
    /// `bl close` the gate child (PR merged, empty deliverable, or rollback —
    /// with the `drop` verb gone, close is the one terminal, §15 bl-65e0).
    fn close_gate(&self, gate: &str) -> Result<()>;
    /// Commit any pending `work/<id>` worktree change so delivery loses nothing.
    fn capture(&self, id: &str, title: &str) -> Result<()>;
    /// Does `work/<id>` exist AND differ from `base`? (false = empty deliverable)
    fn has_changes(&self, id: &str, base: &str) -> Result<bool>;
    /// Push `work/<id>` and open/update its PR; return the PR URL (the §6 human
    /// hint balls forwards on stdout).
    fn push_pr(&self, id: &str, title: &str, base: &str) -> Result<String>;
    /// `unclaim.post`: close the PR and delete the remote `work/<id>` branch.
    fn teardown(&self, id: &str) -> Result<()>;
    /// `sync`: has the PR for `parent`'s `work/<parent>` branch merged?
    fn pr_merged(&self, parent: &str) -> Result<bool>;
    /// `sync`: every `(parent, gate)` link the territory still holds.
    fn pending_gates(&self) -> Result<Vec<(String, String)>>;
}

/// The resolved facts one per-ball hook acts on, assembled by the binary edge
/// from the §7 wire + config. `sync` carries none of these (it has no single
/// ball) and so does not build a `Ctx`.
#[derive(Default)]
pub struct Ctx {
    pub id: String,
    pub title: String,
    /// The PR base (per-task `target_branch`, else config default). Only
    /// `close.pre` reads it; other ops leave it empty.
    pub base: String,
}

/// Run the hook `(op, phase)` — or its rollback when `rolling_back` — against
/// `forge`. Returns an optional stdout line (the PR URL hint, §6). Unknown hooks
/// no-op (the plugin acts only where it is wired), as does `rollback close.pre`
/// (§14).
pub fn dispatch(
    op: &str,
    phase: &str,
    rolling_back: bool,
    forge: &dyn Forge,
    ctx: &Ctx,
) -> Result<Option<String>> {
    match (op, phase, rolling_back) {
        ("claim", "post", false) => {
            if forge.recall_gate(&ctx.id)?.is_none() {
                let gate = forge.create_gate(&ctx.id, &ctx.title)?;
                forge.remember_gate(&ctx.id, &gate)?;
            }
            Ok(None)
        }
        ("claim", "post", true) => {
            release_gate(forge, &ctx.id)?;
            Ok(None)
        }
        ("close", "pre", false) => close_pre(forge, ctx),
        ("unclaim", "post", false) => {
            forge.teardown(&ctx.id)?;
            Ok(None)
        }
        ("sync", "post", false) => {
            sync(forge)?;
            Ok(None)
        }
        // rollback close.pre = no-op (in-review state stays); any unwired hook too.
        _ => Ok(None),
    }
}

/// `close.pre`: capture, then push+PR if there is something to review, else
/// auto-resolve the gate (the §11 empty deliverable).
fn close_pre(forge: &dyn Forge, ctx: &Ctx) -> Result<Option<String>> {
    forge.capture(&ctx.id, &ctx.title)?;
    if forge.has_changes(&ctx.id, &ctx.base)? {
        Ok(Some(forge.push_pr(&ctx.id, &ctx.title, &ctx.base)?))
    } else {
        release_gate(forge, &ctx.id)?;
        Ok(None)
    }
}

/// `sync.post`: for each remembered `parent → gate`, close the gate once its PR
/// has merged, then forget the link.
fn sync(forge: &dyn Forge) -> Result<()> {
    for (parent, gate) in forge.pending_gates()? {
        if forge.pr_merged(&parent)? {
            forge.close_gate(&gate)?;
            forge.forget_gate(&parent)?;
        }
    }
    Ok(())
}

/// Close a remembered gate and forget the link — the shared shape behind the
/// empty-deliverable close and the claim rollback. A parent with no remembered
/// gate (a non-deliverable never gated) is a clean no-op.
fn release_gate(forge: &dyn Forge, parent: &str) -> Result<()> {
    if let Some(gate) = forge.recall_gate(parent)? {
        forge.close_gate(&gate)?;
        forge.forget_gate(parent)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "forge_tests.rs"]
mod tests;
