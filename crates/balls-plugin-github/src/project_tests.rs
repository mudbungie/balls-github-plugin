use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A `git -C <cwd>` Command with the ambient `GIT_*` vars stripped (the
/// pre-commit hook exports them; a child `git` must not inherit them).
fn gitcmd(cwd: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(cwd);
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"] {
        c.env_remove(var);
    }
    c
}

fn git(cwd: &Path, args: &[&str]) {
    assert!(gitcmd(cwd).args(args).output().unwrap().status.success(), "git {args:?}");
}

fn fake_bl(dir: &Path) -> PathBuf {
    let p = dir.join("bl");
    // print an id for `create`, succeed silently otherwise
    std::fs::write(&p, "#!/bin/sh\ncase \"$1\" in create) echo bl-gate;; esac\nexit 0\n").unwrap();
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

struct Harness {
    dir: tempfile::TempDir,
    root: PathBuf,
    bare: PathBuf,
    state: PathBuf,
    bl: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "user.email", "t@e"]);
        std::fs::write(root.join("README"), "hi\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "init"]);
        let bare = dir.path().join("remote.git");
        git(dir.path(), &["init", "--bare", "-q", bare.to_str().unwrap()]);
        let state = dir.path().join("state");
        let bl = fake_bl(dir.path());
        Self { dir, root, bare, state, bl }
    }

    /// A `work/bl-1` worktree carrying a committed change (so it is pushable).
    fn with_work(&self) {
        let wt = self.dir.path().join("wt");
        git(&self.root, &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "work/bl-1"]);
        std::fs::write(wt.join("change.txt"), "delivered\n").unwrap();
        git(&wt, &["add", "-A"]);
        git(&wt, &["commit", "-qm", "work"]);
    }

    fn project(&self, api_base: &str) -> Project {
        let config: PluginConfig =
            serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":"{api_base}"}}"#)).unwrap();
        Project::new(
            &config,
            "tok",
            self.bare.to_string_lossy().into_owned(),
            Git::at(&self.root),
            Bl::new(&self.bl, &self.root, "alice"),
            Territory::new(&self.state, "balls-plugin-github", "/proj"),
        )
    }
}

#[test]
fn subject_carries_the_id_tag() {
    assert_eq!(subject("Fix it", "bl-9"), "Fix it [bl-9]");
}

#[test]
fn gate_lifecycle_uses_bl_and_territory() {
    let h = Harness::new();
    let p = h.project("http://x");
    assert_eq!(p.create_gate("bl-p", "T").unwrap(), "bl-gate");
    p.remember_gate("bl-p", "bl-gate").unwrap();
    assert_eq!(p.recall_gate("bl-p").unwrap().as_deref(), Some("bl-gate"));
    assert_eq!(p.pending_gates().unwrap(), vec![("bl-p".to_string(), "bl-gate".to_string())]);
    p.close_gate("bl-gate").unwrap();
    p.drop_gate("bl-gate").unwrap();
    p.forget_gate("bl-p").unwrap();
    assert_eq!(p.recall_gate("bl-p").unwrap(), None);
}

#[test]
fn capture_and_has_changes_track_the_work_branch() {
    let h = Harness::new();
    let p = h.project("http://x");
    assert!(!p.has_changes("bl-1", "main").unwrap()); // no branch yet
    h.with_work();
    p.capture("bl-1", "T").unwrap(); // already committed -> no-op
    assert!(p.has_changes("bl-1", "main").unwrap());
}

#[test]
fn push_pr_creates_a_pr_when_none_exists() {
    let h = Harness::new();
    h.with_work();
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into())).with_status(200).with_body("[]").create();
    s.mock("POST", "/repos/o/n/pulls")
        .with_status(201)
        .with_body(r#"{"number":5,"html_url":"https://gh/pr/5"}"#)
        .create();
    let p = h.project(&s.url());
    assert_eq!(p.push_pr("bl-1", "Do it", "main").unwrap(), "https://gh/pr/5");
    assert!(gitcmd(&h.bare).args(["rev-parse", "--verify", "refs/heads/work/bl-1"]).output().unwrap().status.success());
}

#[test]
fn push_pr_reuses_an_existing_pr() {
    let h = Harness::new();
    h.with_work();
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body(r#"[{"number":8,"html_url":"https://gh/pr/8"}]"#)
        .create();
    let p = h.project(&s.url());
    assert_eq!(p.push_pr("bl-1", "Do it", "main").unwrap(), "https://gh/pr/8");
}

#[test]
fn teardown_closes_pr_and_deletes_remote_branch() {
    let h = Harness::new();
    h.with_work();
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body(r#"[{"number":3,"html_url":"u"}]"#)
        .create();
    s.mock("PATCH", "/repos/o/n/pulls/3").with_status(200).with_body(r#"{"number":3,"html_url":"u"}"#).create();
    let p = h.project(&s.url());
    p.git.push(&p.push_url, "bl-1").unwrap(); // get the branch onto the remote first
    p.teardown("bl-1").unwrap();
    assert!(!gitcmd(&h.bare).args(["rev-parse", "--verify", "refs/heads/work/bl-1"]).output().unwrap().status.success());
}

#[test]
fn teardown_skips_close_when_no_pr() {
    let h = Harness::new();
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into())).with_status(200).with_body("[]").create();
    let p = h.project(&s.url());
    p.teardown("bl-1").unwrap(); // no PR, no branch on remote — clean no-op
}

#[test]
fn pr_merged_reports_merge_state() {
    let cases = [(r#"[{"number":1,"html_url":"u","merged":true}]"#, true), (r#"[{"number":1,"html_url":"u","merged":false}]"#, false), ("[]", false)];
    for (body, want) in cases {
        let h = Harness::new();
        let mut s = mockito::Server::new();
        s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into())).with_status(200).with_body(body).create();
        let p = h.project(&s.url());
        assert_eq!(p.pr_merged("bl-1").unwrap(), want, "body={body}");
    }
}
