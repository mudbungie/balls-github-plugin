use crate::config::PluginConfig;
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use std::path::Path;

pub fn run(config_path: &Path, auth_dir: &Path) -> Result<()> {
    let config = PluginConfig::load(config_path)?;
    let token = auth::load_token(auth_dir)?;
    GithubClient::new(config.api_base(), &token, USER_AGENT).current_user()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_at(dir: &Path, api: &str) -> std::path::PathBuf {
        let p = dir.join("github-issues.json");
        std::fs::write(&p, format!(r#"{{"repo":"o/n","api_base":{:?}}}"#, api)).unwrap();
        p
    }

    #[test]
    fn ok_when_token_valid() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/user")
            .with_status(200)
            .with_body(r#"{"login":"x"}"#)
            .create();
        let dir = tempfile::tempdir().unwrap();
        let cfg = config_at(dir.path(), &s.url());
        auth::save_token(dir.path(), "t").unwrap();
        run(&cfg, dir.path()).unwrap();
    }

    #[test]
    fn err_when_token_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config_at(dir.path(), "https://api.github.com");
        assert!(run(&cfg, dir.path()).is_err());
    }

    #[test]
    fn err_when_token_rejected() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/user").with_status(401).create();
        let dir = tempfile::tempdir().unwrap();
        let cfg = config_at(dir.path(), &s.url());
        auth::save_token(dir.path(), "t").unwrap();
        assert!(run(&cfg, dir.path()).is_err());
    }
}
