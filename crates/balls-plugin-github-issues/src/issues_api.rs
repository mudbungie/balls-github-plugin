//! Issues plugin's GitHub Issues endpoints and Task accessors. The
//! shared `GithubClient` handles auth + status mapping; this module
//! adds the issue-shaped requests and the projection accessors that
//! read `task.external.github-issues.*`.

use crate::config::PROJECTION_PREFIX;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::Task;
use serde::Deserialize;
use serde_json::Value;

/// The subset of a GitHub Issue this plugin needs. Other fields the
/// API returns are ignored (serde drops unknown keys; SPEC §13).
#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub html_url: String,
    pub state: String,
}

fn issues_url(client: &GithubClient, owner: &str, name: &str) -> String {
    format!("{}/repos/{}/{}/issues", client.api_base(), owner, name)
}

pub fn create_issue(
    client: &GithubClient,
    owner: &str,
    name: &str,
    title: &str,
    body: &str,
) -> Result<Issue> {
    let payload = serde_json::json!({ "title": title, "body": body });
    let url = issues_url(client, owner, name);
    let resp = GithubClient::check(
        client
            .auth(client.http().post(&url))
            .json(&payload)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn patch_issue(
    client: &GithubClient,
    owner: &str,
    name: &str,
    number: u64,
    title: &str,
    body: &str,
    state: &str,
) -> Result<Issue> {
    let payload = serde_json::json!({
        "title": title, "body": body, "state": state,
    });
    let url = format!("{}/{}", issues_url(client, owner, name), number);
    let resp = GithubClient::check(
        client
            .auth(client.http().patch(&url))
            .json(&payload)
            .send()?,
    )?;
    Ok(resp.json()?)
}

/// Accessors over `external.github-issues.*` — the projection this
/// plugin owns. Implemented as a trait on shared `Task` so the
/// projection literal lives only in this crate (matching the forge
/// plugin's `ForgeTaskExt` pattern).
pub trait IssuesTaskExt {
    fn issue_blob(&self) -> Option<&Value>;
    fn issue_number(&self) -> Option<u64>;
    fn last_synced_status(&self) -> Option<&str>;
    /// The RFC3339 `synced_at` last written by push (B3). Used by
    /// B4a's classify for loop avoidance: a GH issue whose
    /// `updated_at` is older than (or equal to) this is one we
    /// already saw and reflects our own write coming back via the
    /// API.
    fn synced_at(&self) -> Option<&str>;
}

impl IssuesTaskExt for Task {
    fn issue_blob(&self) -> Option<&Value> {
        // `task.external` is keyed by the participant name; the
        // "external." prefix is already implied. Strip both the
        // leading "external." and the trailing "." from
        // PROJECTION_PREFIX to recover the participant key.
        let key = PROJECTION_PREFIX
            .strip_prefix("external.")
            .and_then(|s| s.strip_suffix('.'))
            .expect("PROJECTION_PREFIX is 'external.<name>.'");
        self.external.get(key).and_then(|v| v.get("issue"))
    }

    fn issue_number(&self) -> Option<u64> {
        self.issue_blob()
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_u64())
    }

    fn last_synced_status(&self) -> Option<&str> {
        self.issue_blob()
            .and_then(|v| v.get("last_synced_status"))
            .and_then(|v| v.as_str())
    }

    fn synced_at(&self) -> Option<&str> {
        self.issue_blob()
            .and_then(|v| v.get("synced_at"))
            .and_then(|v| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-issues-test";

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn accessors_default_to_none() {
        let t = task(r#"{"id":"bl-x","title":"t","status":"open"}"#);
        assert!(t.issue_blob().is_none());
        assert!(t.issue_number().is_none());
        assert!(t.last_synced_status().is_none());
    }

    #[test]
    fn accessors_read_projection() {
        let t = task(
            r#"{"id":"bl-p","title":"t","status":"open",
                "external":{"github-issues":{"issue":{
                    "number":5,"url":"u","state":"open",
                    "source":"balls","synced_at":"t","last_synced_status":"open"
                }}}}"#,
        );
        assert!(t.issue_blob().is_some());
        assert_eq!(t.issue_number(), Some(5));
        assert_eq!(t.last_synced_status(), Some("open"));
    }

    #[test]
    fn create_issue_round_trip() {
        let mut s = mockito::Server::new();
        s.mock("POST", "/repos/o/n/issues")
            .with_status(201)
            .with_body(
                r#"{"number":12,"html_url":"https://gh/i/12","state":"open"}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let issue = create_issue(&c, "o", "n", "Title [bl-1]", "body").unwrap();
        assert_eq!(issue.number, 12);
        assert_eq!(issue.state, "open");
    }

    #[test]
    fn create_issue_propagates_api_error() {
        let mut s = mockito::Server::new();
        s.mock("POST", "/repos/o/n/issues")
            .with_status(422)
            .with_body("invalid")
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(create_issue(&c, "o", "n", "x", "y").is_err());
    }

    #[test]
    fn patch_issue_state_round_trip() {
        let mut s = mockito::Server::new();
        s.mock("PATCH", "/repos/o/n/issues/9")
            .with_status(200)
            .with_body(
                r#"{"number":9,"html_url":"u","state":"closed"}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let issue = patch_issue(&c, "o", "n", 9, "T", "b", "closed").unwrap();
        assert_eq!(issue.state, "closed");
    }

    #[test]
    fn patch_issue_propagates_api_error() {
        let mut s = mockito::Server::new();
        s.mock("PATCH", "/repos/o/n/issues/9")
            .with_status(404)
            .with_body("gone")
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(patch_issue(&c, "o", "n", 9, "T", "b", "open").is_err());
    }
}
