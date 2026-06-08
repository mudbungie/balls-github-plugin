//! The PROJECT-repo git acts the forge hooks need (§11), against the invocation
//! path: find the `work/<id>` worktree and capture its pending work, decide
//! whether that branch carries changes (the empty deliverable), push it to the
//! forge remote, and delete the remote branch on teardown. Every act is
//! idempotent — it recomputes from `(root, id)` and checks refs/filesystem
//! first, so a re-run is a no-op rather than an error.

use balls_github_shared::error::{PluginError, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// git against one project-repo root.
pub struct Git {
    root: PathBuf,
}

impl Git {
    #[must_use]
    pub fn at(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    /// The `work/<id>` branch name — the one place the convention lives.
    #[must_use]
    pub fn branch(id: &str) -> String {
        format!("work/{id}")
    }

    /// `git -C <cwd>` with the ambient `GIT_*` vars stripped: this plugin always
    /// means "the project repo at this path", never an inherited git context (a
    /// pre-commit hook or a CI wrapper exports `GIT_DIR`/`GIT_WORK_TREE`, which
    /// would silently retarget every command).
    fn base(cwd: &Path) -> Command {
        let mut c = Command::new("git");
        c.arg("-C").arg(cwd);
        for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"] {
            c.env_remove(var);
        }
        c
    }

    fn run(cwd: &Path, args: &[&str]) -> Result<String> {
        let out = Self::base(cwd).args(args).output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(PluginError::Other(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    /// Run for the exit code alone (a predicate): `Ok(true)` on exit 0.
    fn ok(cwd: &Path, args: &[&str]) -> Result<bool> {
        Ok(Self::base(cwd)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?
            .success())
    }

    /// The worktree checked out on `work/<id>`, if any.
    pub fn worktree_of(&self, id: &str) -> Result<Option<PathBuf>> {
        let porcelain = Self::run(&self.root, &["worktree", "list", "--porcelain"])?;
        Ok(parse_worktree(&porcelain, &Self::branch(id)))
    }

    /// Commit any pending change in the `work/<id>` worktree so delivery loses
    /// nothing (idempotent: a clean tree, or no worktree at all, is a no-op).
    pub fn capture(&self, id: &str, subject: &str) -> Result<()> {
        let Some(wt) = self.worktree_of(id)? else {
            return Ok(());
        };
        Self::run(&wt, &["add", "-A"])?;
        if Self::ok(&wt, &["diff", "--cached", "--quiet"])? {
            return Ok(()); // nothing staged — clean
        }
        Self::run(&wt, &["commit", "-m", subject])?;
        Ok(())
    }

    /// Does `work/<id>` exist AND differ from `base`? `false` = the empty
    /// deliverable (branch never made, or no changes vs base, §11).
    pub fn has_changes(&self, id: &str, base: &str) -> Result<bool> {
        let branch = Self::branch(id);
        if !Self::ok(&self.root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])? {
            return Ok(false);
        }
        Ok(!Self::ok(&self.root, &["diff", "--quiet", base, &branch])?)
    }

    /// Push `work/<id>` to `url` (force — the work branch is the claimant's own,
    /// re-pushed each delivery), so the forge can open/refresh the PR.
    pub fn push(&self, url: &str, id: &str) -> Result<()> {
        let branch = Self::branch(id);
        let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
        Self::run(&self.root, &["push", url, &refspec])?;
        Ok(())
    }

    /// Delete the remote `work/<id>` branch on teardown (best-effort — an
    /// already-gone branch is not an error).
    pub fn delete_remote(&self, url: &str, id: &str) -> Result<()> {
        let branch = Self::branch(id);
        Self::ok(&self.root, &["push", url, "--delete", &format!("refs/heads/{branch}")])?;
        Ok(())
    }
}

/// The `tasks/<id>.md` paths the op changed in the change worktree at `cwd` —
/// how a `close.pre` hook recovers the id off the pre wire (§7). Reads the
/// working tree against `HEAD`, so a staged-or-unstaged deletion both show.
pub fn changed_task_paths(cwd: &Path) -> Result<Vec<String>> {
    let out = Git::run(cwd, &["diff", "--name-only", "HEAD", "--", "tasks"])?;
    Ok(out.lines().map(str::to_string).collect())
}

/// Find the worktree path checked out on `branch` in `git worktree list
/// --porcelain` output (a `worktree <path>` line followed by `branch <ref>`).
fn parse_worktree(porcelain: &str, branch: &str) -> Option<PathBuf> {
    let want = format!("refs/heads/{branch}");
    let mut current: Option<PathBuf> = None;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current = Some(PathBuf::from(p));
        } else if line.strip_prefix("branch ") == Some(want.as_str()) {
            return current;
        }
    }
    None
}

#[cfg(test)]
#[path = "git_ops_tests.rs"]
mod tests;
