use super::*;

/// A `git -C <cwd>` Command with the ambient `GIT_*` vars stripped — the
/// pre-commit hook exports `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`, which a
/// child `git` would otherwise inherit and apply to our tempdir fixtures.
fn gitcmd(cwd: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(cwd);
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"] {
        c.env_remove(var);
    }
    c
}

fn git(cwd: &Path, args: &[&str]) {
    let ok = gitcmd(cwd).args(args).output().unwrap().status.success();
    assert!(ok, "git {args:?} failed in {cwd:?}");
}

/// A project repo on `main` with one commit; returns its root.
fn repo(dir: &Path) -> PathBuf {
    let root = dir.join("proj");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("README"), "hi\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    root
}

/// Add a `work/<id>` worktree (branched off main) at `<dir>/wt-<id>`.
fn add_worktree(root: &Path, dir: &Path, id: &str) -> PathBuf {
    let wt = dir.join(format!("wt-{id}"));
    git(root, &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", &Git::branch(id)]);
    wt
}

#[test]
fn worktree_of_finds_the_branch_or_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    let g = Git::at(&root);
    assert_eq!(g.worktree_of("bl-1").unwrap(), None);

    let wt = add_worktree(&root, dir.path(), "bl-1");
    assert_eq!(g.worktree_of("bl-1").unwrap().unwrap().canonicalize().unwrap(), wt.canonicalize().unwrap());
    // a different id is not matched
    assert_eq!(g.worktree_of("bl-2").unwrap(), None);
}

#[test]
fn capture_commits_pending_work_then_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    let wt = add_worktree(&root, dir.path(), "bl-1");
    let g = Git::at(&root);

    std::fs::write(wt.join("new.txt"), "work\n").unwrap();
    g.capture("bl-1", "T [bl-1]").unwrap();
    // committed: a second capture is a no-op (clean tree)
    g.capture("bl-1", "T [bl-1]").unwrap();
    let log = gitcmd(&root).args(["log", "--oneline", &Git::branch("bl-1")]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).lines().count(), 2);
}

#[test]
fn capture_without_a_worktree_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    Git::at(&root).capture("bl-absent", "s").unwrap();
}

#[test]
fn has_changes_distinguishes_empty_from_real() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    let g = Git::at(&root);
    // branch absent
    assert!(!g.has_changes("bl-1", "main").unwrap());

    let wt = add_worktree(&root, dir.path(), "bl-1");
    // branch == main, no work
    assert!(!g.has_changes("bl-1", "main").unwrap());

    std::fs::write(wt.join("new.txt"), "work\n").unwrap();
    g.capture("bl-1", "T [bl-1]").unwrap();
    assert!(g.has_changes("bl-1", "main").unwrap());
}

#[test]
fn push_then_delete_remote_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    let wt = add_worktree(&root, dir.path(), "bl-1");
    std::fs::write(wt.join("new.txt"), "work\n").unwrap();
    Git::at(&root).capture("bl-1", "T [bl-1]").unwrap();

    let bare = dir.path().join("remote.git");
    git(dir.path(), &["init", "--bare", "-q", bare.to_str().unwrap()]);
    let url = bare.to_str().unwrap();
    let g = Git::at(&root);

    g.push(url, "bl-1").unwrap();
    assert!(gitcmd(&bare).args(["rev-parse", "--verify", "refs/heads/work/bl-1"]).output().unwrap().status.success());

    g.delete_remote(url, "bl-1").unwrap();
    assert!(!gitcmd(&bare).args(["rev-parse", "--verify", "refs/heads/work/bl-1"]).output().unwrap().status.success());
    // deleting an already-gone branch is a clean no-op
    g.delete_remote(url, "bl-1").unwrap();
}

#[test]
fn changed_task_paths_lists_staged_deletions() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    std::fs::create_dir(root.join("tasks")).unwrap();
    std::fs::write(root.join("tasks/bl-7.md"), "+++\n+++\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "add task"]);
    // stage a deletion (what `close` does)
    git(&root, &["rm", "-q", "tasks/bl-7.md"]);
    assert_eq!(changed_task_paths(&root).unwrap(), vec!["tasks/bl-7.md".to_string()]);
}

#[test]
fn push_failure_surfaces_git_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo(dir.path());
    let err = Git::at(&root).push("/no/such/remote.git", "bl-1").unwrap_err().to_string();
    assert!(err.contains("git push"), "{err}");
}
