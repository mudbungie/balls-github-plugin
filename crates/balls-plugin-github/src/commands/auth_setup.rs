use crate::config::PluginConfig;
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use std::io::{BufRead, Write};
use std::path::Path;

pub fn run(config_path: &Path, auth_dir: &Path) -> Result<()> {
    let config = PluginConfig::load(config_path)?;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stderr();
    run_with_io(&config, auth_dir, &mut input, &mut out)
}

/// Token comes from stdin (not a TTY prompt) so the whole flow is scriptable
/// and testable. Prompts go to `output` (stderr) to keep stdout clean for the
/// plugin JSON protocol.
pub fn run_with_io(
    config: &PluginConfig,
    auth_dir: &Path,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<()> {
    writeln!(output, "GitHub plugin auth setup for {}", config.repo())?;
    write!(output, "Paste a GitHub token (PAT) and press Enter: ")?;
    output.flush()?;

    let mut line = String::new();
    input.read_line(&mut line)?;
    let token = line.trim();
    if token.is_empty() {
        return Err(PluginError::Auth("empty token".into()));
    }

    let login = GithubClient::new(config.api_base(), token, USER_AGENT).current_user()?;
    auth::save_token(auth_dir, token)?;
    writeln!(output, "Authenticated as {}. Token stored.", login)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn config(api: &str) -> PluginConfig {
        serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":{:?}}}"#, api)).unwrap()
    }

    #[test]
    fn stores_token_on_success() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/user")
            .with_status(200)
            .with_body(r#"{"login":"octocat"}"#)
            .create();
        let dir = tempfile::tempdir().unwrap();
        let mut input = Cursor::new(b"ghp_secret\n".to_vec());
        let mut out = Vec::new();
        run_with_io(&config(&s.url()), dir.path(), &mut input, &mut out).unwrap();
        assert_eq!(auth::load_token(dir.path()).unwrap(), "ghp_secret");
        assert!(String::from_utf8(out).unwrap().contains("octocat"));
    }

    #[test]
    fn empty_token_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = Cursor::new(b"\n".to_vec());
        let mut out = Vec::new();
        let err = run_with_io(
            &config("https://api.github.com"),
            dir.path(),
            &mut input,
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("empty token"));
    }

    #[test]
    fn invalid_token_is_not_stored() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/user").with_status(401).create();
        let dir = tempfile::tempdir().unwrap();
        let mut input = Cursor::new(b"bad\n".to_vec());
        let mut out = Vec::new();
        assert!(run_with_io(&config(&s.url()), dir.path(), &mut input, &mut out).is_err());
        assert!(auth::load_token(dir.path()).is_err());
    }
}
