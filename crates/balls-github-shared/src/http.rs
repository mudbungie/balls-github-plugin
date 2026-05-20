//! Thin GitHub REST client: header construction (Authorization, Accept,
//! User-Agent), status-code mapping, and the one endpoint both plugins
//! need at the protocol level (`GET /user`, the auth-check probe).
//!
//! Anything PR- or issue-specific lives in the relevant plugin crate
//! and uses this client's `auth(...)`/`check(...)` helpers to add its
//! own endpoints. The orphan-rule cost of free-functions-over-foreign-
//! types is intentional: it forces plugin-specific HTTP code into
//! plugin-specific crates, which keeps this crate's projection-boundary
//! invariant (see `lib.rs`).

use crate::error::{PluginError, Result};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

pub struct GithubClient {
    http: Client,
    api_base: String,
    token: String,
    user_agent: String,
}

impl GithubClient {
    pub fn new(api_base: &str, token: &str, user_agent: &str) -> Self {
        Self {
            http: Client::new(),
            api_base: api_base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            user_agent: user_agent.to_string(),
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    /// Decorate a `RequestBuilder` with the standard auth + JSON +
    /// user-agent headers. Plugin-specific endpoint helpers compose
    /// this with their own URL and body.
    pub fn auth(&self, builder: RequestBuilder) -> RequestBuilder {
        builder
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", &self.user_agent)
    }

    /// Map a non-2xx response into a `PluginError::GithubApi` carrying
    /// the status code and the body verbatim for diagnostics. 2xx
    /// passes through unchanged.
    pub fn check(resp: Response) -> Result<Response> {
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

    /// `GET /user` — validates the token and returns the authenticated
    /// login. Used by every plugin's `auth-setup` and `auth-check`.
    pub fn current_user(&self) -> Result<String> {
        let url = format!("{}/user", self.api_base);
        let resp = Self::check(self.auth(self.http.get(&url)).send()?)?;
        Ok(resp.json::<User>()?.login)
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
        let c = GithubClient::new(&s.url(), "t", "balls-test");
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
        let c = GithubClient::new(&s.url(), "t", "balls-test");
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
        let c = GithubClient::new("http://127.0.0.1:1", "t", "balls-test");
        assert!(matches!(
            c.current_user().unwrap_err(),
            PluginError::Http(_)
        ));
    }

    #[test]
    fn api_base_trimmed() {
        let c = GithubClient::new("https://api.example/", "t", "ua");
        assert_eq!(c.api_base(), "https://api.example");
    }

    #[test]
    fn auth_decorates_request() {
        // Smoke test that auth() produces a builder we can finalize;
        // mockito then verifies the headers landed.
        let mut s = mockito::Server::new();
        let m = s
            .mock("GET", "/echo")
            .match_header("Authorization", "Bearer secret")
            .match_header("User-Agent", "ua-check")
            .with_status(200)
            .with_body("ok")
            .create();
        let c = GithubClient::new(&s.url(), "secret", "ua-check");
        let url = format!("{}/echo", c.api_base());
        let resp = c.auth(c.http().get(&url)).send().unwrap();
        GithubClient::check(resp).unwrap();
        m.assert();
    }
}
