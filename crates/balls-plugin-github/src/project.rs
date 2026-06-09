//! The production [`Forge`]: wires the project-repo git ([`Git`]), the `bl`
//! shell-back ([`Bl`]), the plugin's XDG territory ([`Territory`]), and the
//! GitHub Pulls API ([`pr_api`]) into the seam [`crate::forge::dispatch`] drives.
//! Each method is a thin delegation — the policy lives in `forge`, the mechanics
//! in those four collaborators, so this layer carries no branching of its own
//! beyond the find-or-create / find-or-skip PR shapes.

use crate::bl_ops::Bl;
use crate::config::PluginConfig;
use crate::forge::Forge;
use crate::git_ops::Git;
use crate::pr_api;
use crate::scratch::Territory;
use crate::USER_AGENT;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;

/// The wired-up forge: one repo, one GitHub repository, one territory.
pub struct Project {
    git: Git,
    bl: Bl,
    territory: Territory,
    client: GithubClient,
    owner: String,
    name: String,
    push_url: String,
}

impl Project {
    /// Wire a [`Project`]. `push_url` is the authenticated git URL the edge
    /// derives from config + token ([`pr_api::push_url`]). `config` is already
    /// validated (its `repo` split is `owner/name`) by [`PluginConfig::load`],
    /// so the split here cannot fail — an invariant, not a runtime branch.
    #[must_use]
    pub fn new(config: &PluginConfig, token: &str, push_url: String, git: Git, bl: Bl, territory: Territory) -> Self {
        let (owner, name) = config.owner_name().expect("config repo validated at load (owner/name)");
        Self {
            git,
            bl,
            territory,
            client: GithubClient::new(config.api_base(), token, USER_AGENT),
            owner: owner.to_string(),
            name: name.to_string(),
            push_url,
        }
    }
}

/// The delivery commit / PR / gate subject: `<title> [<id>]` — the same tag
/// shape the direct delivery plugin squashes under (§11), so a forge-delivered
/// task is greppable the same way.
fn subject(title: &str, id: &str) -> String {
    format!("{title} [{id}]")
}

impl Forge for Project {
    fn create_gate(&self, parent: &str, title: &str) -> Result<String> {
        self.bl.create_gate(parent, title)
    }
    fn remember_gate(&self, parent: &str, gate: &str) -> Result<()> {
        self.territory.remember_gate(parent, gate)
    }
    fn recall_gate(&self, parent: &str) -> Result<Option<String>> {
        self.territory.recall_gate(parent)
    }
    fn forget_gate(&self, parent: &str) -> Result<()> {
        self.territory.forget_gate(parent)
    }
    fn close_gate(&self, gate: &str) -> Result<()> {
        self.bl.close(gate)
    }
    fn drop_gate(&self, gate: &str) -> Result<()> {
        self.bl.drop(gate)
    }
    fn capture(&self, id: &str, title: &str) -> Result<()> {
        self.git.capture(id, &subject(title, id))
    }
    fn has_changes(&self, id: &str, base: &str) -> Result<bool> {
        self.git.has_changes(id, base)
    }
    fn push_pr(&self, id: &str, title: &str, base: &str) -> Result<String> {
        self.git.push(&self.push_url, id)?;
        let head = Git::branch(id);
        let pr = if let Some(existing) = pr_api::find_pr(&self.client, &self.owner, &self.name, &head)? { existing } else {
            let qualified = format!("{}:{head}", self.owner);
            let body = format!("Delivers {id} via balls forge delivery.");
            pr_api::create_pr(&self.client, &self.owner, &self.name, &subject(title, id), &qualified, base, &body)?
        };
        Ok(pr.html_url)
    }
    fn teardown(&self, id: &str) -> Result<()> {
        let head = Git::branch(id);
        if let Some(pr) = pr_api::find_pr(&self.client, &self.owner, &self.name, &head)? {
            pr_api::close_pr(&self.client, &self.owner, &self.name, pr.number)?;
        }
        self.git.delete_remote(&self.push_url, id)
    }
    fn pr_merged(&self, parent: &str) -> Result<bool> {
        let head = Git::branch(parent);
        Ok(pr_api::find_pr(&self.client, &self.owner, &self.name, &head)?.is_some_and(|p| p.merged))
    }
    fn pending_gates(&self) -> Result<Vec<(String, String)>> {
        self.territory.pending_gates()
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
