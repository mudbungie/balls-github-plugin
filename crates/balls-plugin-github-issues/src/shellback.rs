//! Shelling back to `bl` (bl-613d) — how the pull side mutates balls under the
//! no-return-channel protocol.
//!
//! There is no return channel (§7): the plugin cannot hand balls a list of
//! changes to apply. Instead it drives the PUBLIC verb surface — `bl create` /
//! `bl update` / `bl close` — exactly as the §6 invocation-tree cap anticipates
//! ("a plugin shelling back"). Each shelled verb is a normal sealed op that runs
//! its own hooks (the tracker pushes it). The depth odometer (`BALLS_PLUGIN_DEPTH`)
//! bounds it; a sync→create→create.post chain is shallow and never re-triggers
//! `sync`, so it never nears the cap.
//!
//! Every pull-driven verb carries [`IMPORT_GUARD`] in its env: we are importing
//! FROM GitHub, so the push handler those verbs fire (`create.post`/`close.post`)
//! must NOT echo the change back OUT to GitHub (that is the loop, and the
//! duplicate-issue bug). The guard is this plugin's private re-entrancy signal,
//! not a core concept.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::Command;

/// Env var set on every pull-driven `bl` call; the dispatcher skips the push
/// handler when it is present (resolved at the edge as `Env::importing`).
pub const IMPORT_GUARD: &str = "BALLS_GITHUB_ISSUES_IMPORTING";

/// Git env vars balls' own git invocation exports; they must be stripped from a
/// shelled `bl` or its git ops target THIS plugin's caller repo instead of the
/// project (the forge plugin's bl-189b gotcha).
const GIT_VARS: &[&str] =
    &["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"];

/// A `bl` driver bound to a binary and the project directory it runs in. The cwd
/// MUST be the invocation path (the project root), not the plugin's sync cwd
/// (the store checkout), so `bl` resolves the same clone bundle (§1).
pub struct Bl {
    bin: OsString,
    cwd: PathBuf,
    actor: String,
}

impl Bl {
    #[must_use]
    pub fn new(bin: OsString, cwd: PathBuf, actor: String) -> Self {
        Self { bin, cwd, actor }
    }

    fn command(&self) -> Command {
        let mut c = Command::new(&self.bin);
        c.current_dir(&self.cwd).env(IMPORT_GUARD, "1");
        for var in GIT_VARS {
            c.env_remove(var);
        }
        c
    }

    /// Run `bl <args…>`, returning captured stdout on success. A non-zero exit
    /// is an error carrying the verb + stderr — the sync aborts (§6), surfacing
    /// the failure rather than silently dropping the import.
    fn run(&self, args: &[&str]) -> io::Result<String> {
        let out = self.command().args(args).output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(io::Error::other(format!(
                "bl {} failed: {}",
                args.first().copied().unwrap_or(""),
                String::from_utf8_lossy(&out.stderr).trim(),
            )))
        }
    }

    /// `bl create --body B [-t tag…] -- "title"` → the new ball id (parsed from
    /// stdout). Tags become balls tags (GitHub labels mirror in as tags).
    ///
    /// The title is GitHub-sourced — UNTRUSTED — and rides a positional, so it
    /// goes behind the `--` end-of-options separator: a hostile `-`-leading
    /// title (`--as …`) stays a title instead of hijacking a flag (the bl-d31f
    /// core seam, mirroring core's own arg-injection guard 938e75a0). `body`
    /// and the labels are untrusted too, but they ride as FLAG VALUES — the
    /// parser consumes the token after `--body`/`-t` unconditionally, so they
    /// are never read as flags and need no guard.
    pub fn create(&self, title: &str, body: &str, tags: &[String]) -> io::Result<String> {
        let mut args: Vec<String> = vec!["create".into(), "--body".into(), body.into()];
        for t in tags {
            args.push("-t".into());
            args.push(t.clone());
        }
        args.push("--as".into());
        args.push(self.actor.clone());
        args.push("--".into());
        args.push(title.into());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let stdout = self.run(&refs)?;
        extract_id(&stdout)
            .ok_or_else(|| io::Error::other(format!("bl create printed no id: {stdout:?}")))
    }

    /// `bl update <id> -t <tag>` — add a tag (the only inward field edit the
    /// verb surface allows besides priority/extras). Used for the `deferred`
    /// external-delete policy. The `id` positional needs no `--` guard: every id
    /// reaching here is [`extract_id`]-minted (strict `bl-<hex>`) or
    /// marker-parsed (`crate::marker::strip` requires the `bl-` prefix), so the
    /// token always leads with `b`, never `-` — it cannot read as a flag. `tag`
    /// is a flag value (see [`Bl::create`]).
    pub fn add_tag(&self, id: &str, tag: &str) -> io::Result<()> {
        self.run(&["update", id, "-t", tag, "--as", &self.actor]).map(drop)
    }

    /// `bl close <id>` — the inward close mirror. As with [`Bl::add_tag`], the
    /// id is `bl-`-prefixed by both producing grammars, never `-`-leading.
    pub fn close(&self, id: &str) -> io::Result<()> {
        self.run(&["close", id, "--as", &self.actor]).map(drop)
    }
}

/// Scan `bl` stdout for a `bl-<hex>` id, returning the last one (create prints a
/// confirmation line carrying the new id; core log lines go to stderr). The id
/// grammar is [`crate::marker::is_id`] — the one minted-shape gate shared with
/// the title-marker parse (§16 — the id scheme is fixed, no config).
#[must_use]
pub fn extract_id(stdout: &str) -> Option<String> {
    stdout
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .rfind(|tok| crate::marker::is_id(tok))
        .map(str::to_string)
}

/// Resolve the `bl` binary at the process edge: `$BALLS_BIN` if set (the test
/// seam), else `bl` from PATH.
#[must_use]
pub fn resolve_bin(env_bin: Option<OsString>) -> OsString {
    env_bin.unwrap_or_else(|| OsString::from("bl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    #[test]
    fn extract_id_finds_the_last_marker() {
        assert_eq!(extract_id("create bl-1a2b\n").as_deref(), Some("bl-1a2b"));
        assert_eq!(extract_id("noise bl-aaaa then bl-bbbb").as_deref(), Some("bl-bbbb"));
        assert_eq!(extract_id("nothing here"), None);
        assert_eq!(extract_id("bl-xyz too short hex"), None); // non-hex
    }

    #[test]
    fn resolve_bin_prefers_env() {
        assert_eq!(resolve_bin(Some(OsString::from("/x/bl"))), OsString::from("/x/bl"));
        assert_eq!(resolve_bin(None), OsString::from("bl"));
    }

    /// Write an executable fake `bl` that records argv + cwd + the guard env and
    /// emits `script_stdout`, exiting `code`.
    fn fake_bl(dir: &Path, code: i32, script_stdout: &str) -> PathBuf {
        let log = dir.join("calls.log");
        let path = dir.join("bl");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> {log}\necho \"guard=$BALLS_GITHUB_ISSUES_IMPORTING\" >> {log}\npwd >> {log}\nprintf '%s' '{script_stdout}'\nexit {code}\n",
            log = log.display(),
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn create_runs_bl_with_guard_and_parses_id() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bl(dir.path(), 0, "create bl-9f9f\\n");
        let bl = Bl::new(bin.into(), dir.path().to_path_buf(), "tester".into());
        let id = bl.create("Title", "body", &["bug".into()]).unwrap();
        assert_eq!(id, "bl-9f9f");

        let log = std::fs::read_to_string(dir.path().join("calls.log")).unwrap();
        // The untrusted title rides behind `--` (end-of-options), after the flags.
        assert!(log.contains("create --body body -t bug --as tester -- Title"), "argv: {log}");
        assert!(log.contains("guard=1"), "guard not set: {log}");
    }

    #[test]
    fn a_hostile_dash_leading_title_stays_a_positional() {
        // A GitHub-sourced title like `--as evil` must land AFTER the `--`
        // separator, where bl reads it as a title, never as a flag.
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bl(dir.path(), 0, "create bl-9f9f\\n");
        let bl = Bl::new(bin.into(), dir.path().to_path_buf(), "tester".into());
        bl.create("--as evil", "b", &[]).unwrap();
        let log = std::fs::read_to_string(dir.path().join("calls.log")).unwrap();
        assert!(log.contains("--as tester -- --as evil"), "argv: {log}");
    }

    #[test]
    fn create_errors_when_no_id_printed() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bl(dir.path(), 0, "ok but no id");
        let bl = Bl::new(bin.into(), dir.path().to_path_buf(), "tester".into());
        assert!(bl.create("T", "b", &[]).is_err());
    }

    #[test]
    fn nonzero_exit_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bl(dir.path(), 1, "");
        let bl = Bl::new(bin.into(), dir.path().to_path_buf(), "tester".into());
        assert!(bl.close("bl-1").is_err());
    }

    #[test]
    fn add_tag_and_close_pass_argv() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bl(dir.path(), 0, "");
        let bl = Bl::new(bin.into(), dir.path().to_path_buf(), "tester".into());
        bl.add_tag("bl-1", "deferred").unwrap();
        bl.close("bl-2").unwrap();
        let log = std::fs::read_to_string(dir.path().join("calls.log")).unwrap();
        assert!(log.contains("update bl-1 -t deferred"));
        assert!(log.contains("close bl-2"));
    }
}
