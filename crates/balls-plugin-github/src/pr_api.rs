//! Forge-specific PR endpoints and Task accessors. These live here,
//! not in `balls_github_shared`, because they touch the forge's own
//! `external.github.*` projection and the GitHub Pulls API — both are
//! plugin-specific. The shared crate's `GithubClient` provides auth +
//! status mapping; the request building and response decoding are this
//! crate's responsibility.

use crate::types::PrRef;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::Task;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct GitRef {
    #[serde(default)]
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct Pr {
    pub number: u64,
    pub html_url: String,
    pub head: GitRef,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
}

impl Pr {
    pub fn to_ref(&self, target_branch: &str) -> PrRef {
        PrRef {
            number: self.number,
            url: self.html_url.clone(),
            head_sha: self.head.sha.clone(),
            target_branch: target_branch.to_string(),
        }
    }
}

fn pulls_url(client: &GithubClient, owner: &str, name: &str) -> String {
    format!("{}/repos/{}/{}/pulls", client.api_base(), owner, name)
}

pub fn find_pr(
    client: &GithubClient,
    owner: &str,
    name: &str,
    head_branch: &str,
) -> Result<Option<Pr>> {
    let head = format!("{}:{}", owner, head_branch);
    let url = pulls_url(client, owner, name);
    let resp = GithubClient::check(
        client
            .auth(client.http().get(&url))
            .query(&[("head", head.as_str()), ("state", "all")])
            .send()?,
    )?;
    Ok(resp.json::<Vec<Pr>>()?.into_iter().next())
}

pub fn create_pr(
    client: &GithubClient,
    owner: &str,
    name: &str,
    title: &str,
    head: &str,
    base: &str,
    body: &str,
) -> Result<Pr> {
    let payload = serde_json::json!({
        "title": title, "head": head, "base": base, "body": body,
    });
    let url = pulls_url(client, owner, name);
    let resp = GithubClient::check(
        client
            .auth(client.http().post(&url))
            .json(&payload)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn get_pr(client: &GithubClient, owner: &str, name: &str, number: u64) -> Result<Pr> {
    let url = format!("{}/{}", pulls_url(client, owner, name), number);
    let resp = GithubClient::check(client.auth(client.http().get(&url)).send()?)?;
    Ok(resp.json()?)
}

/// Forge-specific accessors on the shared `Task`. The methods read
/// `external.github.*` — this trait is what makes the projection
/// boundary load-bearing: anything that needs to peek into the forge
/// projection imports `ForgeTaskExt`, which only this crate can
/// provide.
pub trait ForgeTaskExt {
    fn pull_request(&self) -> Option<&Value>;
    fn pr_number(&self) -> Option<u64>;
    fn gate_child_id(&self) -> Option<&str>;
}

impl ForgeTaskExt for Task {
    fn pull_request(&self) -> Option<&Value> {
        self.external
            .get("github")
            .and_then(|v| v.get("pull_request"))
    }

    fn pr_number(&self) -> Option<u64> {
        self.pull_request()
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_u64())
    }

    fn gate_child_id(&self) -> Option<&str> {
        self.links
            .iter()
            .find(|l| l.link_type == "gates")
            .map(|l| l.target.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-test";

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn forge_task_ext_defaults() {
        let t = task(r#"{"id":"bl-x","title":"t","status":"open"}"#);
        assert!(t.pull_request().is_none());
        assert!(t.pr_number().is_none());
        assert!(t.gate_child_id().is_none());
    }

    #[test]
    fn forge_task_ext_populated() {
        let t = task(
            r#"{"id":"bl-p","title":"t","status":"review",
                "links":[{"link_type":"relates_to","target":"bl-z"},
                         {"link_type":"gates","target":"bl-g"}],
                "external":{"github":{"pull_request":{"number":7}}}}"#,
        );
        assert!(t.pull_request().is_some());
        assert_eq!(t.pr_number(), Some(7));
        assert_eq!(t.gate_child_id(), Some("bl-g"));
    }

    #[test]
    fn pr_number_absent_when_not_numeric() {
        let t = task(
            r#"{"id":"bl-p","title":"t","status":"review",
                "external":{"github":{"pull_request":{"number":"x"}}}}"#,
        );
        assert_eq!(t.pr_number(), None);
    }

    #[test]
    fn find_pr_empty_then_one() {
        let mut s = mockito::Server::new();
        let none = s
            .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
            .with_status(200)
            .with_body("[]")
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(find_pr(&c, "o", "n", "work/bl-1").unwrap().is_none());
        none.assert();

        let mut s2 = mockito::Server::new();
        s2.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
            .with_status(200)
            .with_body(
                r#"[{"number":4,"html_url":"u","head":{"ref":"work/bl-1","sha":"s"},
                     "base":{"ref":"main"}}]"#,
            )
            .create();
        let c2 = GithubClient::new(&s2.url(), "t", UA);
        let pr = find_pr(&c2, "o", "n", "work/bl-1").unwrap().unwrap();
        assert_eq!(pr.number, 4);
        assert_eq!(pr.to_ref("main").head_sha, "s");
    }

    #[test]
    fn create_pr_ok_and_error() {
        let mut s = mockito::Server::new();
        s.mock("POST", "/repos/o/n/pulls")
            .with_status(201)
            .with_body(
                r#"{"number":9,"html_url":"u","head":{"ref":"h","sha":"z"},
                    "base":{"ref":"main"}}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let pr = create_pr(&c, "o", "n", "T [bl-1]", "h", "main", "b").unwrap();
        assert_eq!(pr.number, 9);

        let mut s2 = mockito::Server::new();
        s2.mock("POST", "/repos/o/n/pulls")
            .with_status(422)
            .with_body("no commits")
            .create();
        let c2 = GithubClient::new(&s2.url(), "t", UA);
        assert!(create_pr(&c2, "o", "n", "t", "h", "main", "b").is_err());
    }

    #[test]
    fn get_pr_merged() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/pulls/12")
            .with_status(200)
            .with_body(
                r#"{"number":12,"html_url":"u","head":{"ref":"h","sha":"z"},
                    "base":{"ref":"main"},"merged":true,
                    "merge_commit_sha":"deadbeef"}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let pr = get_pr(&c, "o", "n", 12).unwrap();
        assert!(pr.merged);
        assert_eq!(pr.merge_commit_sha.as_deref(), Some("deadbeef"));
    }
}
