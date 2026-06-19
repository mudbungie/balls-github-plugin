//! The forge plugin's POLICY — the §6/§7 hook-dispatch matrix, subtask model.
//!
//! Forge is NOT a delivery variant (§11, bl-7bfe): it never hooks `close.pre`,
//! and there is no plugin-side submission step. The plugin shrinks to two
//! moments around an ordinary close-blocker gate child (§10):
//!
//! - **`claim.post`** — mint the **review gate child** of the claimed task: one
//!   `bl create --parent <id> --blocks close` — an EXPLICIT close-gate edge
//!   (since bl-5d9a `--subtask-of` gates the parent's CLAIM, not its close, so
//!   bl-788e's one-word close-gate sugar was superseded; the canonical spelling
//!   matches bl-chore), carrying the plugin-namespaced preserved key (§3 extras)
//!   that joins gate → parent. Minting SKIPS when the
//!   claimed task is itself one of the plugin's gate children (no
//!   gates-for-gates) and when a standing open gate for this parent already
//!   exists (an unclaim-and-reclaim reuses it). The minted id is the hook's
//!   stdout product (§6).
//! - **`sync.post`** — for each open gate child (the preserved-key scan over
//!   `bl list --json`), check the parent's PR by its `work/<parent>` head
//!   branch; merged ⇒ `bl close` the gate child, unblocking the parent's close.
//!
//! SUBMISSION IS GIT-NATIVE WORK: the worker pushes `work/<id>` and opens the
//! PR themselves, `[bl-id]` in the PR title — core delivery's tag-scan
//! (bl-430e) then recognizes the squash-merge as delivered and skips the local
//! squash. An empty deliverable's gate has no auto-resolve moment: its claimant
//! closes it by hand. Abandoning a forge-gated task means closing or
//! `--no-needs`-unlinking the gate first (both skill-doc lines, bl-7bfe).
//!
//! **Rollback (§14):** `rollback claim.post` deletes the just-minted gate child
//! — close is the one retirement, so "delete" is `bl close`. The gate is
//! DERIVED (the same preserved-key scan), never scratch: every input is
//! recomputable, so the plugin holds no id-keyed state at all.
//!
//! The matrix is pure: every side effect goes through the [`Forge`] seam, so it
//! is unit-tested against a fake without a repo, a network, or a real `bl`.

use crate::wire::Gate;
use balls_github_shared::error::Result;

/// The protocol self-description (`<bin> protocol`, §6): this plugin speaks
/// protocol 1 and handles the two ops it hooks. balls reads it at install time,
/// validates the wiring against it, and never persists it.
pub const PROTOCOL_JSON: &str = r#"{"protocol":[1],"ops":["claim","sync"]}"#;

/// The side-effecting acts the forge hooks need, behind a seam so [`dispatch`]
/// is testable without a real `bl` or network. The real impl is
/// [`crate::project::Project`].
pub trait Forge {
    /// Every OPEN gate child of this plugin's — the preserved-key scan over
    /// `bl list --json` (closed gates have no file, so absence = resolved).
    fn open_gates(&self) -> Result<Vec<Gate>>;
    /// Mint the review gate child of `parent` (`bl create --parent <id>
    /// --blocks close` — an explicit close-gate edge, bl-5d9a — + the join key),
    /// returning the minted id.
    fn mint_gate(&self, parent: &str, title: &str) -> Result<String>;
    /// `bl close` the gate child (resolve on merge, or the mint rollback —
    /// close is the one retirement, §10).
    fn close_gate(&self, gate: &str, note: &str) -> Result<()>;
    /// The merged PR's URL for `parent`'s `work/<parent>` head branch, if any
    /// PR exists AND has merged (`None`: no PR yet, or still open).
    fn merged_pr(&self, parent: &str) -> Result<Option<String>>;
}

/// The resolved facts a `claim.post` hook acts on, assembled by the binary edge
/// from the §7 post wire. `sync` carries none of these (it has no single ball)
/// and uses the default.
#[derive(Default)]
pub struct Ctx {
    /// The claimed task's id (the sealed `bl-id` trailer).
    pub id: String,
    /// The claimed task's title (the gate child's subject source).
    pub title: String,
    /// `Some(parent)` when the claimed ball itself carries the plugin's join
    /// key — it IS one of the plugin's gate children, so minting skips.
    pub gate_of: Option<String>,
}

/// Run the hook `(op, phase)` — or its rollback when `rolling_back` — against
/// `forge`. Returns the optional stdout product (§6: the minted gate child's
/// id; sync's resolved-gate lines). Unknown hooks no-op (the plugin acts only
/// where it is wired).
pub fn dispatch(
    op: &str,
    phase: &str,
    rolling_back: bool,
    forge: &dyn Forge,
    ctx: &Ctx,
) -> Result<Option<String>> {
    match (op, phase, rolling_back) {
        ("claim", "post", false) => claim_post(forge, ctx),
        ("claim", "post", true) => rollback_claim(forge, &ctx.id),
        ("sync", "post", false) => sync(forge),
        _ => Ok(None),
    }
}

/// `claim.post`: mint the review gate child, unless the claimed task is itself
/// a gate child of ours (no gates-for-gates) or a standing open gate for it
/// already exists (idempotent reclaim reuse).
fn claim_post(forge: &dyn Forge, ctx: &Ctx) -> Result<Option<String>> {
    if ctx.gate_of.is_some() || gate_of_parent(forge, &ctx.id)?.is_some() {
        return Ok(None);
    }
    Ok(Some(forge.mint_gate(&ctx.id, &ctx.title)?))
}

/// `rollback claim.post`: delete (close) the just-minted gate child, derived by
/// the same key scan — no scratch (§14). No open gate is a clean no-op (the
/// mint never happened, or was already undone).
fn rollback_claim(forge: &dyn Forge, parent: &str) -> Result<Option<String>> {
    if let Some(gate) = gate_of_parent(forge, parent)? {
        forge.close_gate(&gate, "review gate withdrawn: the claim rolled back")?;
    }
    Ok(None)
}

/// `sync.post`: close every open gate child whose parent's PR has merged. The
/// returned lines are the §6 human channel — one per resolved gate.
fn sync(forge: &dyn Forge) -> Result<Option<String>> {
    let mut lines = Vec::new();
    for g in forge.open_gates()? {
        if let Some(url) = forge.merged_pr(&g.parent)? {
            forge.close_gate(&g.id, &format!("PR merged: {url}"))?;
            lines.push(format!("{} resolved: {} merged ({url})", g.id, g.parent));
        }
    }
    Ok((!lines.is_empty()).then(|| lines.join("\n")))
}

/// The open gate child gating `parent`, if any — the derived (never stored)
/// `parent → gate` join.
fn gate_of_parent(forge: &dyn Forge, parent: &str) -> Result<Option<String>> {
    Ok(forge.open_gates()?.into_iter().find(|g| g.parent == parent).map(|g| g.id))
}

#[cfg(test)]
#[path = "forge_tests.rs"]
mod tests;
