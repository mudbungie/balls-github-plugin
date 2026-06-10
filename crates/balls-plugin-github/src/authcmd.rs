//! `auth-setup` / `auth-check` — the two MANUAL (non-hook) subcommands a human
//! runs to provision the GitHub token. They are NOT §6 ops (balls never
//! dispatches them); the protocol self-describe omits them. The token is the
//! one secret, stored under the plugin's territory `auth/` dir (§1), mode 0600,
//! by the shared `auth` module. Core never reads it.

use crate::{Env, USER_AGENT};
use balls_github_shared::auth;
use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use std::io::Write;
use std::path::PathBuf;

/// The auth dir in the plugin's §1 territory:
/// `$XDG_STATE_HOME/balls/plugins/<name>/auth`. The token is per-machine, not
/// per-project, so no invocation path keys it.
#[must_use]
pub fn auth_dir(env: &Env) -> PathBuf {
    env.state_home.join("balls").join("plugins").join(&env.plugin_name).join("auth")
}

/// Read the token from stdin (already passed as `token`), validate it against
/// `api_base` (`GET /user`), and store it. Validation first means a bad token is
/// rejected before it is written.
pub fn setup(env: &Env, api_base: &str, token: &str, out: &mut dyn Write) -> Result<()> {
    let login = GithubClient::new(api_base, token, USER_AGENT).current_user()?;
    auth::save_token(&auth_dir(env), api_base, token)?;
    writeln!(out, "authenticated as {login}; token stored")?;
    Ok(())
}

/// Verify the stored token against `api_base`.
pub fn check(env: &Env, api_base: &str, out: &mut dyn Write) -> Result<()> {
    let token = auth::load_token(&auth_dir(env), api_base)?;
    let login = GithubClient::new(api_base, &token, USER_AGENT).current_user()?;
    writeln!(out, "ok: authenticated as {login}")?;
    Ok(())
}

#[cfg(test)]
#[path = "authcmd_tests.rs"]
mod tests;
