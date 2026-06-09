//! The §6 process surface (bl-613d): `protocol` self-describe, the two
//! human-run auth subcommands, and `<op> <phase>` hook routing with the §7
//! payload on stdin. All policy lives in the handler modules; this routes.
//!
//! balls never reads a return channel — stdout here is diagnostics, the hooks
//! contribute by shelling `bl` (`crate::pull`) or calling GitHub
//! (`crate::push`). A handler error becomes a non-zero exit, which aborts the op
//! (§6).

use std::io::{Read, Write};
use std::path::Path;

use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;

use crate::base::Base;
use crate::config::{config_path, PluginConfig};
use crate::shellback::Bl;
use crate::territory::Xdg;
use crate::wire::Payload;
use crate::{push, pull};

/// User-Agent for this plugin's GitHub calls.
pub const USER_AGENT: &str = "balls-plugin-github-issues";

/// The §6 self-description. balls never persists it; install reads it once to
/// validate the binding.
pub const PROTOCOL_JSON: &str =
    r#"{"protocol":1,"ops":["create","update","close","drop","sync"]}"#;

/// Host-resolved process context, gathered once at the edge (no env reads in the
/// handlers).
pub struct Env<'a> {
    pub xdg: Xdg,
    /// The directory a human ran an auth subcommand from (the project root).
    pub cwd: &'a Path,
    /// The `bl` binary the pull side shells (`$BALLS_BIN` or `bl`).
    pub bl_bin: std::ffi::OsString,
    /// `true` when the import guard env is set — a pull-driven nested op, so the
    /// push handler suppresses itself.
    pub importing: bool,
    /// Default API base for the auth subcommands (no binding to read config from).
    pub default_api_base: String,
}

/// Dispatch `args`, returning a process exit code.
pub fn run(args: &[String], stdin: &mut impl Read, out: &mut impl Write, env: &Env) -> i32 {
    match dispatch(args, stdin, out, env) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("github-issues: {e}");
            1
        }
    }
}

fn dispatch(args: &[String], stdin: &mut impl Read, out: &mut impl Write, env: &Env) -> Result<()> {
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["protocol"] => {
            writeln!(out, "{PROTOCOL_JSON}")?;
            Ok(())
        }
        ["auth-setup"] => auth_setup(stdin, &env.default_api_base, env),
        ["auth-check"] => auth_check(&env.default_api_base, env),
        ["adopt", legacy_dir] => adopt_cmd(legacy_dir, env),
        [op, phase] => hook(op, phase, stdin, env),
        _ => Err(PluginError::Other(
            "usage: github-issues protocol | auth-setup | auth-check | adopt <legacy-tasks-dir> | <op> <phase>".into(),
        )),
    }
}

/// `auth-setup`: read a token from stdin, validate it, store it in territory.
fn auth_setup(stdin: &mut impl Read, api_base: &str, env: &Env) -> Result<()> {
    let mut token = String::new();
    stdin.read_to_string(&mut token)?;
    let token = token.trim();
    if token.is_empty() {
        return Err(PluginError::Auth("empty token on stdin".into()));
    }
    let client = GithubClient::new(api_base, token, USER_AGENT);
    let login = client.current_user()?;
    let dir = territory_for_cwd(env)?;
    auth::save_token(&dir, token)?;
    eprintln!("github-issues: token stored for {login}");
    Ok(())
}

/// `auth-check`: validate the stored token; exit code is the answer.
fn auth_check(api_base: &str, env: &Env) -> Result<()> {
    let dir = territory_for_cwd(env)?;
    let token = auth::load_token(&dir)?;
    GithubClient::new(api_base, &token, USER_AGENT).current_user()?;
    Ok(())
}

/// `adopt <legacy-tasks-dir>`: the one-time §16 cutover step (bl-2a81). Seed the
/// reconciliation base from a legacy task store's `external.github-issues.issue.
/// number` blobs so the first greenfield `sync` re-adopts existing issues with
/// zero dups. Offline; territory keys on the cwd, like the auth subcommands.
fn adopt_cmd(legacy_dir: &str, env: &Env) -> Result<()> {
    let territory = territory_for_cwd(env)?;
    let summary = crate::adopt::adopt(Path::new(legacy_dir), &territory)?;
    eprintln!(
        "github-issues: adopted {} legacy issue link(s); skipped {}",
        summary.seeded, summary.skipped
    );
    Ok(())
}

fn territory_for_cwd(env: &Env) -> Result<std::path::PathBuf> {
    let invocation = env.cwd.to_str().ok_or_else(|| PluginError::Other("cwd is not utf-8".into()))?;
    Ok(env.xdg.territory(invocation))
}

/// Route one `<op> <phase>` hook.
fn hook(op: &str, phase: &str, stdin: &mut impl Read, env: &Env) -> Result<()> {
    let payload: Payload = serde_json::from_reader(stdin)?;
    if payload.op != op {
        return Err(PluginError::Other(format!("argv op {op:?} != payload op {:?}", payload.op)));
    }
    // Only the slots this plugin acts in pay the config/auth cost.
    let acts = matches!(
        (op, phase),
        ("create" | "update" | "close" | "drop", "post") | ("sync", "post")
    );
    if !acts {
        return Ok(());
    }
    if op != "sync" && env.importing {
        return Ok(()); // a pull-driven nested op — do not echo back out
    }

    let landing = &payload.binding.landing;
    let cfg = PluginConfig::load(&config_path(landing))?;
    let (owner, name) = cfg
        .owner_name()
        .ok_or_else(|| PluginError::Config("repo is not owner/name".into()))?;
    let territory = env.xdg.territory(&payload.binding.invocation_path);
    let token = auth::load_token(&territory)?;
    let client = GithubClient::new(cfg.api_base(), &token, USER_AGENT);
    let mut base = Base::load(&territory)?;

    if op == "sync" {
        let store_dir = Path::new(&payload.binding.store);
        let bl = Bl::new(
            env.bl_bin.clone(),
            payload.binding.invocation_path.clone().into(),
            payload.actor.clone(),
        );
        pull::pull(&client, owner, name, &cfg, &mut base, store_dir, &territory, &bl)?;
    } else {
        let store_dir = Path::new(&payload.binding.store);
        push::push(&payload, &client, owner, name, &mut base, &territory, store_dir)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
