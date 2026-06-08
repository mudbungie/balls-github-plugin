//! The GitHub Pulls endpoints the forge plugin drives, plus the authenticated
//! git push URL. These live here, not in `balls_github_shared`, because they
//! touch the forge's own concern (pull requests); the shared crate provides only
//! auth + status mapping (the projection-boundary invariant, see workspace
//! README).
//!
//! The plugin is STATELESS across ops (§11): it never stores the PR number. It
//! re-finds the PR each time by its head branch (`work/<id>`), so there is no
//! `external.github.*` projection to keep in sync — exactly the "delivery IS the
//! tag, not a field" discipline applied to the PR.

use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use serde::Deserialize;

/// The slice of a GitHub PR the forge plugin reads back.
#[derive(Debug, Deserialize)]
pub struct Pr {
    pub number: u64,
    pub html_url: String,
    #[serde(default)]
    pub merged: bool,
}

fn pulls_url(client: &GithubClient, owner: &str, name: &str) -> String {
    format!("{}/repos/{}/{}/pulls", client.api_base(), owner, name)
}

/// Find the (open or merged) PR whose head is `work/<id>`, if any.
pub fn find_pr(client: &GithubClient, owner: &str, name: &str, head_branch: &str) -> Result<Option<Pr>> {
    let head = format!("{owner}:{head_branch}");
    let resp = GithubClient::check(
        client
            .auth(client.http().get(pulls_url(client, owner, name)))
            .query(&[("head", head.as_str()), ("state", "all")])
            .send()?,
    )?;
    Ok(resp.json::<Vec<Pr>>()?.into_iter().next())
}

/// Open a PR from `head` into `base`.
pub fn create_pr(
    client: &GithubClient,
    owner: &str,
    name: &str,
    title: &str,
    head: &str,
    base: &str,
    body: &str,
) -> Result<Pr> {
    let payload = serde_json::json!({ "title": title, "head": head, "base": base, "body": body });
    let resp = GithubClient::check(
        client.auth(client.http().post(pulls_url(client, owner, name))).json(&payload).send()?,
    )?;
    Ok(resp.json()?)
}

/// Close the PR `number` (teardown on `bl drop`).
pub fn close_pr(client: &GithubClient, owner: &str, name: &str, number: u64) -> Result<()> {
    let url = format!("{}/{}", pulls_url(client, owner, name), number);
    GithubClient::check(
        client.auth(client.http().patch(&url)).json(&serde_json::json!({ "state": "closed" })).send()?,
    )?;
    Ok(())
}

/// The authenticated git push URL for `repo` (`owner/name`) — token in the URL
/// so the push is self-contained (no ambient credential helper needed). The git
/// host derives from the API base: `api.github.com` → `github.com`; a GitHub
/// Enterprise `https://HOST/api/v3` → `HOST`.
#[must_use]
pub fn push_url(api_base: &str, repo: &str, token: &str) -> String {
    format!("https://x-access-token:{token}@{}/{repo}.git", git_host(api_base))
}

fn git_host(api_base: &str) -> String {
    let after = api_base.trim_start_matches("https://").trim_start_matches("http://");
    let host = after.split_once('/').map_or(after, |(h, _)| h);
    if host == "api.github.com" {
        "github.com".to_string()
    } else {
        host.to_string()
    }
}

#[cfg(test)]
#[path = "pr_api_tests.rs"]
mod tests;
