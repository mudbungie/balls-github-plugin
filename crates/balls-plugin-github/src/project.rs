//! The production [`Forge`]: wires the `bl` shell-back ([`Bl`]) and the GitHub
//! Pulls API ([`pr_api`]) into the seam [`crate::forge::dispatch`] drives. Each
//! method is a thin delegation — the policy lives in `forge`, the mechanics in
//! the two collaborators — so this layer carries no branching beyond the
//! mint's inline cleanup.
//!
//! The plugin is STATELESS across ops (§11/§14): the `parent → gate` join is
//! the plugin-namespaced preserved key on the gate child (scanned back out of
//! `bl list --json`), and the PR is re-found each sync by its `work/<parent>`
//! head branch — no scratch, no `external.github.*` projection, nothing that
//! can drift.

use crate::bl_ops::Bl;
use crate::forge::Forge;
use crate::pr_api::{self, Pr};
use crate::wire::{self, Gate};
use crate::USER_AGENT;
use balls_github_shared::config_base::RepoConfig;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;

/// The wired-up forge: one project's `bl`, one GitHub repository, one join key.
pub struct Project {
    bl: Bl,
    client: GithubClient,
    owner: String,
    name: String,
    /// The plugin-namespaced preserved key (§3) — the plugin's own name, so two
    /// differently-named forge wirings never claim each other's gate children.
    key: String,
}

impl Project {
    /// Wire a [`Project`]. `config` is already validated (its `repo` split is
    /// `owner/name`) by the edge's load, so the split here cannot fail — an
    /// invariant, not a runtime branch.
    #[must_use]
    pub fn new(config: &RepoConfig, token: &str, key: String, bl: Bl) -> Self {
        let (owner, name) = config.owner_name().expect("config repo validated at load (owner/name)");
        Self {
            bl,
            client: GithubClient::new(config.api_base(), token, USER_AGENT),
            owner: owner.to_string(),
            name: name.to_string(),
            key,
        }
    }
}

/// The `work/<id>` head-branch convention (§11) — the one place it lives here.
fn branch(id: &str) -> String {
    format!("work/{id}")
}

impl Forge for Project {
    fn open_gates(&self) -> Result<Vec<Gate>> {
        wire::open_gates(&self.bl.list_json()?, &self.key)
    }

    /// Mint = `create --subtask-of` + stamp the join key. If the stamp fails,
    /// the half-minted gate is withdrawn inline (best-effort) before the error
    /// surfaces — §14: a FAILING plugin's own rollback is never called, so it
    /// cleans up before exiting non-zero.
    fn mint_gate(&self, parent: &str, title: &str) -> Result<String> {
        let gate = self.bl.create_gate(parent, title)?;
        if let Err(e) = self.bl.set_extra(&gate, &self.key, parent) {
            let _ = self.bl.close(&gate, "withdrawn: minting the review gate failed");
            return Err(e);
        }
        Ok(gate)
    }

    fn close_gate(&self, gate: &str, note: &str) -> Result<()> {
        self.bl.close(gate, note)
    }

    fn merged_pr(&self, parent: &str) -> Result<Option<String>> {
        let pr = pr_api::find_pr(&self.client, &self.owner, &self.name, &branch(parent))?;
        Ok(pr.filter(Pr::merged).map(|p| p.html_url))
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
