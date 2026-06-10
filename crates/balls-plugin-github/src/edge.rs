//! The process edge — argv → action. It turns the §6 invocation
//! (`<bin> protocol` | `<bin> auth-setup|auth-check [api_base]` | `<bin> <op>
//! <phase>`) into a call on the [`crate::forge`] policy, building the production
//! [`Project`] seam from the §7 wire + the injected [`Env`]. All env reads
//! happen before this (the bl-bfa8 rule); the pure helpers below are unit-tested
//! and the integration path is covered by the binary tests in `tests/cli.rs`.

use crate::bl_ops::Bl;
use crate::forge::{self, Ctx};
use crate::project::Project;
use crate::wire::{self, Wire};
use crate::{authcmd, Env};
use balls_github_shared::config_base::{load_json, RepoConfig};
use balls_github_shared::error::{PluginError, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Dispatch one invocation. `stdin` is the §7 wire for a hook, or the token for
/// `auth-setup` (ignored otherwise).
pub fn handle(args: &[String], env: &Env, stdin: &str, out: &mut dyn Write) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("protocol") => {
            writeln!(out, "{}", forge::PROTOCOL_JSON)?;
            Ok(())
        }
        Some("auth-setup") => authcmd::setup(env, api_base(args), stdin.trim(), out),
        Some("auth-check") => authcmd::check(env, api_base(args), out),
        Some(op) => {
            let phase = args.get(1).ok_or_else(usage)?;
            hook(op, phase, env, stdin, out)
        }
        None => Err(usage()),
    }
}

/// Run a hook: build the seam, then apply the [`forge`] matrix.
fn hook(op: &str, phase: &str, env: &Env, stdin: &str, out: &mut dyn Write) -> Result<()> {
    let w = Wire::parse(stdin).map_err(|e| PluginError::Other(format!("bad §7 wire: {e}")))?;
    let config: RepoConfig = load_json(&config_path(&w.binding.landing, &env.plugin_name))?;
    config.validate()?;
    let token = balls_github_shared::auth::load_token(&authcmd::auth_dir(env), config.api_base())?;
    let bl = Bl::new(&env.bl_program, Path::new(&w.binding.invocation_path), &w.actor);
    let project = Project::new(&config, &token, env.plugin_name.clone(), bl);
    let ctx = ctx_of(op, &w, &env.plugin_name)?;
    let line = forge::dispatch(op, phase, w.rolling_back.is_some(), &project, &ctx)?;
    emit(line, out)
}

/// The per-ball facts off the post wire — or the empty [`Ctx`] for `sync`,
/// which is diffless (§13: no single ball, no metadata).
fn ctx_of(op: &str, w: &Wire, key: &str) -> Result<Ctx> {
    if op == "sync" {
        return Ok(Ctx::default());
    }
    let state = w.previous_state.as_ref();
    Ok(Ctx {
        id: wire::sealed_id(w.metadata.as_ref())?,
        title: state.map(|s| s.title.clone()).unwrap_or_default(),
        gate_of: state.and_then(|s| s.extra_str(key)).map(str::to_string),
    })
}

/// The optional `api_base` positional for the auth subcommands; defaults to
/// public GitHub.
fn api_base(args: &[String]) -> &str {
    args.get(1).map_or("https://api.github.com", String::as_str)
}

/// The committed config path on the landing: `<landing>/config/plugins/<name>.json`.
fn config_path(landing: &str, name: &str) -> PathBuf {
    Path::new(landing).join("config").join("plugins").join(format!("{name}.json"))
}

/// Forward the hook's optional stdout product (§6: the minted gate child's id,
/// or sync's resolved-gate lines).
fn emit(line: Option<String>, out: &mut dyn Write) -> Result<()> {
    if let Some(l) = line {
        writeln!(out, "{l}")?;
    }
    Ok(())
}

fn usage() -> PluginError {
    PluginError::Other(
        "usage: <op> <phase> | protocol | auth-setup [api_base] | auth-check [api_base]".into(),
    )
}

#[cfg(test)]
#[path = "edge_tests.rs"]
mod tests;
