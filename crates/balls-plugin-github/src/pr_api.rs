//! The one GitHub Pulls read the forge plugin needs. It lives here, not in
//! `balls_github_shared`, because it touches the forge's own concern (pull
//! requests); the shared crate provides only auth + HTTP (the
//! projection-boundary invariant, see workspace README).
//!
//! The plugin is STATELESS across ops (§11/§14): it never stores the PR
//! number. It re-finds the PR each sync by its head branch (`work/<id>`), so
//! there is no scratch and no projection to keep in sync — exactly the
//! "delivery IS the tag, not a field" discipline applied to the PR.

use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use serde::Deserialize;

/// The slice of a GitHub PR the forge plugin reads back. The LIST endpoint
/// returns `merged_at` (nullable) and never the `merged` boolean (that field
/// exists only on the single-PR GET), so merged-ness is `merged_at.is_some()`.
#[derive(Debug, Deserialize)]
pub struct Pr {
    pub html_url: String,
    #[serde(default)]
    pub merged_at: Option<String>,
}

impl Pr {
    /// Whether the PR has merged (a closed-unmerged PR has no `merged_at`).
    #[must_use]
    pub fn merged(&self) -> bool {
        self.merged_at.is_some()
    }
}

/// Find the PR (any state — `sync` cares about merged ones) whose head is
/// `work/<id>`, if any.
pub fn find_pr(client: &GithubClient, owner: &str, name: &str, head_branch: &str) -> Result<Option<Pr>> {
    let url = format!("{}/repos/{owner}/{name}/pulls", client.api_base());
    let head = format!("{owner}:{head_branch}");
    let resp = GithubClient::check(
        client
            .auth(client.http().get(url))
            .query(&[("head", head.as_str()), ("state", "all")])
            .send()?,
    )?;
    Ok(resp.json::<Vec<Pr>>()?.into_iter().next())
}

#[cfg(test)]
#[path = "pr_api_tests.rs"]
mod tests;
