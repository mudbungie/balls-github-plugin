use crate::error::{PluginError, Result};
use crate::types::PrRef;
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

/// Thin GitHub REST client. `api_base` is injectable so tests point it at a
/// mock server and GitHub Enterprise installs work without code changes.
pub struct GithubClient {
    http: Client,
    api_base: String,
    token: String,
}

impl GithubClient {
    pub fn new(api_base: &str, token: &str) -> Self {
        Self {
            http: Client::new(),
            api_base: api_base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    fn auth(&self, builder: RequestBuilder) -> RequestBuilder {
        builder
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "balls-plugin-github")
    }

    fn check(resp: Response) -> Result<Response> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            Err(PluginError::GithubApi {
                status: status.as_u16(),
                body: resp.text().unwrap_or_default(),
            })
        }
    }

    fn pulls_url(&self, owner: &str, name: &str) -> String {
        format!("{}/repos/{}/{}/pulls", self.api_base, owner, name)
    }

    /// Validates the token and returns the authenticated login.
    pub fn current_user(&self) -> Result<String> {
        let url = format!("{}/user", self.api_base);
        let resp = Self::check(self.auth(self.http.get(&url)).send()?)?;
        Ok(resp.json::<User>()?.login)
    }

    pub fn find_pr(&self, owner: &str, name: &str, head_branch: &str) -> Result<Option<Pr>> {
        let head = format!("{}:{}", owner, head_branch);
        let resp = Self::check(
            self.auth(self.http.get(self.pulls_url(owner, name)))
                .query(&[("head", head.as_str()), ("state", "all")])
                .send()?,
        )?;
        Ok(resp.json::<Vec<Pr>>()?.into_iter().next())
    }

    pub fn create_pr(
        &self,
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
        let resp = Self::check(
            self.auth(self.http.post(self.pulls_url(owner, name)))
                .json(&payload)
                .send()?,
        )?;
        Ok(resp.json()?)
    }

    pub fn get_pr(&self, owner: &str, name: &str, number: u64) -> Result<Pr> {
        let url = format!("{}/{}", self.pulls_url(owner, name), number);
        let resp = Self::check(self.auth(self.http.get(&url)).send()?)?;
        Ok(resp.json()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_user_ok() {
        let mut s = mockito::Server::new();
        let m = s
            .mock("GET", "/user")
            .with_status(200)
            .with_body(r#"{"login":"octocat"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t");
        assert_eq!(c.current_user().unwrap(), "octocat");
        m.assert();
    }

    #[test]
    fn current_user_unauthorized() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/user")
            .with_status(401)
            .with_body("bad creds")
            .create();
        let c = GithubClient::new(&s.url(), "t");
        match c.current_user().unwrap_err() {
            PluginError::GithubApi { status, body } => {
                assert_eq!(status, 401);
                assert_eq!(body, "bad creds");
            }
            e => panic!("unexpected {e}"),
        }
    }

    #[test]
    fn transport_error_maps_to_http() {
        // Nothing listening here: send() yields a reqwest transport error.
        let c = GithubClient::new("http://127.0.0.1:1", "t");
        assert!(matches!(
            c.current_user().unwrap_err(),
            PluginError::Http(_)
        ));
    }

    #[test]
    fn find_pr_empty_then_one() {
        let mut s = mockito::Server::new();
        let none = s
            .mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
            .with_status(200)
            .with_body("[]")
            .create();
        let c = GithubClient::new(&s.url(), "t");
        assert!(c.find_pr("o", "n", "work/bl-1").unwrap().is_none());
        none.assert();

        let mut s2 = mockito::Server::new();
        s2.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
            .with_status(200)
            .with_body(
                r#"[{"number":4,"html_url":"u","head":{"ref":"work/bl-1","sha":"s"},
                     "base":{"ref":"main"}}]"#,
            )
            .create();
        let c2 = GithubClient::new(&s2.url(), "t");
        let pr = c2.find_pr("o", "n", "work/bl-1").unwrap().unwrap();
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
        let c = GithubClient::new(&s.url(), "t");
        let pr = c.create_pr("o", "n", "T [bl-1]", "h", "main", "b").unwrap();
        assert_eq!(pr.number, 9);

        let mut s2 = mockito::Server::new();
        s2.mock("POST", "/repos/o/n/pulls")
            .with_status(422)
            .with_body("no commits")
            .create();
        let c2 = GithubClient::new(&s2.url(), "t");
        assert!(c2.create_pr("o", "n", "t", "h", "main", "b").is_err());
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
        let c = GithubClient::new(&s.url(), "t");
        let pr = c.get_pr("o", "n", 12).unwrap();
        assert!(pr.merged);
        assert_eq!(pr.merge_commit_sha.as_deref(), Some("deadbeef"));
    }
}
