//! The `bl` shell-back — the forge plugin's gate-child lifecycle (§6 bounded
//! shell-back: one level deep, never re-triggering its own op). A plugin has no
//! return channel (§7), so it manages the gate child by RUNNING `bl` itself:
//! `create --subtask-of` to mint it (the bl-788e parent + close-gate sugar),
//! `update` to stamp the join key, `close` to resolve it (the one retirement,
//! §10), and `list --json` to derive the open-gate set.
//!
//! The `bl` program path and the `cwd`/`actor` are injected by the edge (the
//! bl-bfa8 rule: no env reads in the lib), so tests drive a fake `bl` without
//! mutating global env.

use balls_github_shared::error::{PluginError, Result};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A `bl` runner bound to one project `cwd` and `actor`.
pub struct Bl {
    program: PathBuf,
    cwd: PathBuf,
    actor: String,
}

impl Bl {
    #[must_use]
    pub fn new(program: &Path, cwd: &Path, actor: &str) -> Self {
        Self { program: program.to_path_buf(), cwd: cwd.to_path_buf(), actor: actor.to_string() }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let out = self.spawn(args)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(PluginError::Other(format!(
                "bl {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    /// Spawn `bl`, retrying on `ETXTBSY` (`ExecutableFileBusy`). A freshly
    /// installed plugin binary can momentarily be open-for-write elsewhere
    /// (and under parallel tests, fork fd-inheritance briefly makes any
    /// just-written executable look busy), so a bounded retry turns a transient
    /// race into a wait rather than a spurious failure.
    fn spawn(&self, args: &[&str]) -> Result<Output> {
        for _ in 0..50 {
            match self.command(args).output() {
                Err(e) if e.kind() == ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                other => return Ok(other?),
            }
        }
        // Still busy after the bounded wait — surface the last attempt's error.
        Ok(self.command(args).output()?)
    }

    /// `bl` in the project `cwd`, with the ambient `GIT_*` vars stripped so the
    /// nested `bl` operates on its own store, never an inherited git context.
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.program);
        c.current_dir(&self.cwd).args(args);
        for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"] {
            c.env_remove(var);
        }
        c
    }

    /// `bl create --subtask-of <parent>` — mint the review gate child (parent
    /// pointer + reciprocal close-gate in one word, §10), returning the id it
    /// prints on stdout.
    ///
    /// `title` is task-sourced (untrusted) and rides a positional, so it goes
    /// behind the `--` end-of-options separator (the bl-d31f core seam). The
    /// `Review gate: ` prefix already keeps the token from leading with `-`,
    /// but the guard makes the safety structural rather than an accident of
    /// formatting.
    pub fn create_gate(&self, parent: &str, title: &str) -> Result<String> {
        let subject = format!("Review gate: {title}");
        let out =
            self.run(&["create", "--subtask-of", parent, "--as", &self.actor, "--", &subject])?;
        parse_id(&out)
            .ok_or_else(|| PluginError::Other(format!("bl create minted no id (stdout: {out:?})")))
    }

    /// `bl update <id> <key>=<value>` — stamp a preserved extra (§3): the
    /// plugin-namespaced join key on a freshly minted gate child (`create`
    /// takes no key=value extras, so the stamp is its own op).
    pub fn set_extra(&self, id: &str, key: &str, value: &str) -> Result<()> {
        let kv = format!("{key}={value}");
        self.run(&["update", id, &kv, "--as", &self.actor])?;
        Ok(())
    }

    /// `bl close <id> -m <note>` — resolve the gate child.
    pub fn close(&self, id: &str, note: &str) -> Result<()> {
        self.run(&["close", id, "-m", note, "--as", &self.actor])?;
        Ok(())
    }

    /// `bl list --json` — the bedrock projection of every live task, the input
    /// to the open-gate scan ([`crate::wire::open_gates`]).
    pub fn list_json(&self) -> Result<String> {
        self.run(&["list", "--json"])
    }
}

/// `bl create` prints the minted id alone on stdout (lifecycle logs go to
/// stderr), so the first `bl-…` token is the id; tolerant of a trailing newline
/// or a stray leading log line.
fn parse_id(stdout: &str) -> Option<String> {
    stdout.split_whitespace().find(|w| w.starts_with("bl-")).map(str::to_string)
}

#[cfg(test)]
#[path = "bl_ops_tests.rs"]
mod tests;
