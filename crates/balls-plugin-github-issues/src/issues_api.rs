//! The GitHub Issues HTTP surface: create, patch, and the paginated list the
//! pull side classifies. The shared [`GithubClient`] handles auth + status
//! mapping; this module adds the issue-shaped requests.
//!
//! Under the no-return-channel rewrite (bl-613d) there are no Task accessors
//! here — the plugin reads no `external.github-issues.*` blob off the ball; the
//! join is the title marker (`crate::marker`) and the state is the base
//! (`crate::base`).

use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use serde::Deserialize;

/// The slim issue shape returned by create/patch — only what the push path
/// records into the base.
#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub state: String,
}

/// One label name on a listed issue.
#[derive(Debug, Clone, Deserialize)]
pub struct GhLabel {
    pub name: String,
}

/// A listed issue (`GET …/issues`). `pull_request` is present on PRs, which the
/// list drops.
#[derive(Debug, Clone, Deserialize)]
pub struct GhIssue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub labels: Vec<GhLabel>,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

impl GhIssue {
    #[must_use]
    pub fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|l| l.name == name)
    }

    #[must_use]
    pub fn body_str(&self) -> &str {
        self.body.as_deref().unwrap_or("")
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.state == "closed"
    }
}

fn issues_url(client: &GithubClient, owner: &str, name: &str) -> String {
    format!("{}/repos/{}/{}/issues", client.api_base(), owner, name)
}

/// `POST …/issues` — create an issue with `title` + `body`.
pub fn create_issue(
    client: &GithubClient,
    owner: &str,
    name: &str,
    title: &str,
    body: &str,
) -> Result<Issue> {
    let payload = serde_json::json!({ "title": title, "body": body });
    let url = issues_url(client, owner, name);
    let resp = GithubClient::check(client.auth(client.http().post(&url)).json(&payload).send()?)?;
    Ok(resp.json()?)
}

/// `PATCH …/issues/{number}` with exactly the JSON `fields` the caller wants to
/// change. One general patch keeps the surface minimal: push sends
/// `{title,state}` or `{title,body,state}`; the close mirror sends `{state}`;
/// the re-assert sends `{title,body}`. Omitted keys are left untouched by GitHub.
pub fn patch(
    client: &GithubClient,
    owner: &str,
    name: &str,
    number: u64,
    fields: &serde_json::Value,
) -> Result<Issue> {
    let url = format!("{}/{}", issues_url(client, owner, name), number);
    let resp = GithubClient::check(client.auth(client.http().patch(&url)).json(fields).send()?)?;
    Ok(resp.json()?)
}

/// `GET …/issues/{number}` — fetch one issue's current shape. The §16 adoption
/// reads the live title here so it can append the `[bl-xxxx]` marker
/// idempotently (`crate::marker::append` needs the bare title to re-stamp).
pub fn get_issue(client: &GithubClient, owner: &str, name: &str, number: u64) -> Result<GhIssue> {
    let url = format!("{}/{}", issues_url(client, owner, name), number);
    let resp = GithubClient::check(client.auth(client.http().get(&url)).send()?)?;
    Ok(resp.json()?)
}

/// `GET …/issues?state=all` walked to completion (per_page=100, following each
/// `Link rel="next"`). The whole set — load-bearing for the delete-sweep: a
/// truncated listing would flag off-page mirrored tasks as externally deleted
/// (bl-bb66). Any page error propagates, so the caller bails before sweeping a
/// partial set. PRs are dropped.
pub fn list_issues(client: &GithubClient, owner: &str, name: &str) -> Result<Vec<GhIssue>> {
    let mut url = format!(
        "{}/repos/{}/{}/issues?state=all&per_page=100",
        client.api_base(),
        owner,
        name
    );
    let mut all = Vec::new();
    loop {
        let resp = GithubClient::check(client.auth(client.http().get(&url)).send()?)?;
        let next = resp
            .headers()
            .get("link")
            .and_then(|v| v.to_str().ok())
            .and_then(next_page_url);
        let page: Vec<GhIssue> = resp.json()?;
        all.extend(page.into_iter().filter(|i| i.pull_request.is_none()));
        match next {
            Some(n) => url = n,
            None => return Ok(all),
        }
    }
}

/// Parse a GH `Link` header and return the `rel="next"` URL if present.
fn next_page_url(link_header: &str) -> Option<String> {
    link_header.split(',').find_map(|part| {
        let (url_seg, rest) = part.split_once(';')?;
        if rest.split(';').any(|s| s.trim() == r#"rel="next""#) {
            Some(url_seg.trim().trim_start_matches('<').trim_end_matches('>').to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-issues-test";

    #[test]
    fn gh_issue_helpers() {
        let i: GhIssue = serde_json::from_str(
            r#"{"number":1,"title":"t","state":"closed","labels":[{"name":"bug"}]}"#,
        )
        .unwrap();
        assert!(i.has_label("bug"));
        assert!(!i.has_label("nope"));
        assert!(i.is_closed());
        assert_eq!(i.body_str(), "");
    }

    #[test]
    fn create_issue_round_trip() {
        let mut s = mockito::Server::new();
        s.mock("POST", "/repos/o/n/issues")
            .with_status(201)
            .with_body(r#"{"number":12,"html_url":"https://gh/i/12","state":"open"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let issue = create_issue(&c, "o", "n", "Title [bl-1]", "body").unwrap();
        assert_eq!(issue.number, 12);
        assert_eq!(issue.state, "open");
    }

    #[test]
    fn create_issue_propagates_api_error() {
        let mut s = mockito::Server::new();
        s.mock("POST", "/repos/o/n/issues").with_status(422).with_body("invalid").create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(create_issue(&c, "o", "n", "x", "y").is_err());
    }

    #[test]
    fn patch_round_trip() {
        let mut s = mockito::Server::new();
        s.mock("PATCH", "/repos/o/n/issues/9")
            .match_body(r#"{"state":"closed"}"#)
            .with_status(200)
            .with_body(r#"{"number":9,"html_url":"u","state":"closed"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let fields = serde_json::json!({ "state": "closed" });
        assert_eq!(patch(&c, "o", "n", 9, &fields).unwrap().state, "closed");
    }

    #[test]
    fn patch_propagates_api_error() {
        let mut s = mockito::Server::new();
        s.mock("PATCH", "/repos/o/n/issues/9").with_status(404).with_body("gone").create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(patch(&c, "o", "n", 9, &serde_json::json!({"state":"open"})).is_err());
    }

    #[test]
    fn get_issue_round_trip() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/issues/9")
            .with_status(200)
            .with_body(r#"{"number":9,"title":"Legacy","state":"open"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let issue = get_issue(&c, "o", "n", 9).unwrap();
        assert_eq!(issue.title, "Legacy");
        assert_eq!(issue.number, 9);
    }

    #[test]
    fn get_issue_propagates_api_error() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/issues/9").with_status(404).with_body("gone").create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(get_issue(&c, "o", "n", 9).is_err());
    }

    #[test]
    fn list_issues_paginates_and_drops_prs() {
        let mut s = mockito::Server::new();
        let p1 = format!(r#"<{}/page2>; rel="next""#, s.url());
        s.mock("GET", "/repos/o/n/issues?state=all&per_page=100")
            .with_status(200)
            .with_header("link", &p1)
            .with_body(
                r#"[{"number":1,"title":"a","state":"open"},
                    {"number":2,"title":"pr","state":"open","pull_request":{"x":1}}]"#,
            )
            .create();
        s.mock("GET", "/page2")
            .with_status(200)
            .with_body(r#"[{"number":3,"title":"c","state":"closed"}]"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let all = list_issues(&c, "o", "n").unwrap();
        let nums: Vec<u64> = all.iter().map(|i| i.number).collect();
        assert_eq!(nums, [1, 3]); // PR #2 dropped, page 2 followed
    }

    #[test]
    fn list_issues_bails_on_page_error() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/issues?state=all&per_page=100")
            .with_status(500)
            .with_body("boom")
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(list_issues(&c, "o", "n").is_err());
    }

    #[test]
    fn next_page_url_parsing() {
        assert_eq!(
            next_page_url(r#"<https://a/2>; rel="next", <https://a/9>; rel="last""#),
            Some("https://a/2".to_string()),
        );
        assert_eq!(next_page_url(r#"<https://a/9>; rel="last""#), None);
        assert_eq!(next_page_url("garbage"), None);
    }
}
