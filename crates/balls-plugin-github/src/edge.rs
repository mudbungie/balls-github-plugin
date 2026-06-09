//! The process edge — argv → action. It turns the §6 invocation
//! (`<bin> protocol` | `<bin> auth-setup|auth-check [api_base]` | `<bin> <op>
//! <phase>`) into a call on the [`crate::forge`] policy, building the production
//! [`Project`] seam from the §7 wire + the injected [`Env`]. All env reads
//! happen before this (the bl-bfa8 rule); the pure helpers below are unit-tested
//! and the integration path is covered by the binary tests in `tests/cli.rs`.

use crate::bl_ops::Bl;
use crate::config::PluginConfig;
use crate::forge::{self, Ctx};
use crate::git_ops::{self, Git};
use crate::pr_api;
use crate::project::Project;
use crate::scratch::Territory;
use crate::wire::{self, Wire};
use crate::{authcmd, Env};
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

/// Run a delivery hook: build the seam, then apply the [`forge`] matrix.
fn hook(op: &str, phase: &str, env: &Env, stdin: &str, out: &mut dyn Write) -> Result<()> {
    let w = Wire::parse(stdin).map_err(|e| PluginError::Other(format!("bad §7 wire: {e}")))?;
    let config = PluginConfig::load(&config_path(&w.binding.landing, &env.plugin_name))?;
    let auth_dir = Territory::new(&env.state_home, &env.plugin_name, "").auth_dir();
    let token = balls_github_shared::auth::load_token(&auth_dir, config.api_base())?;
    let push = pr_api::push_url(config.api_base(), config.repo(), &token);
    let invocation = Path::new(&w.binding.invocation_path);
    let project = Project::new(
        &config,
        &token,
        push,
        Git::at(invocation),
        Bl::new(&env.bl_program, invocation, &w.actor),
        Territory::new(&env.state_home, &env.plugin_name, &w.binding.invocation_path),
    );

    // `sync` is diffless — it carries no single ball, so it skips id/base.
    if op == "sync" {
        forge::dispatch(op, phase, false, &project, &Ctx::default())?;
        return Ok(());
    }
    let id = wire::resolve_id(w.metadata.as_ref(), || git_ops::changed_task_paths(&env.cwd))?;
    let title = w.current_state.as_ref().map(|s| s.title.clone()).unwrap_or_default();
    let task_target = w.current_state.as_ref().and_then(|s| s.target_branch.clone());
    let base = resolve_base(task_target, config.target_branch.clone(), op)?;
    let ctx = Ctx { id, title, base };
    let line = forge::dispatch(op, phase, w.rolling_back.is_some(), &project, &ctx)?;
    emit(line, out)
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

/// The PR base: per-task `target_branch` wins, else the config default. `close`
/// (the only op that opens a PR) REQUIRES one — a forge PR cannot guess a base
/// (§11); other ops never read it, so an absent base defaults empty.
fn resolve_base(task_target: Option<String>, config_target: Option<String>, op: &str) -> Result<String> {
    let base = task_target.or(config_target);
    if op == "close" {
        base.ok_or_else(|| {
            PluginError::Config("no target_branch: set it per-task or in config (a forge PR needs a base)".into())
        })
    } else {
        Ok(base.unwrap_or_default())
    }
}

/// Forward the hook's optional stdout line (the PR URL hint, §6).
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
