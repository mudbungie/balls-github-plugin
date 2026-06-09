//! End-to-end tests: run the actual binary so `main` and the `edge` dispatch are
//! exercised (and counted by coverage). The forge plugin shells back to `bl`
//! (faked via `BALLS_BL`) and never needs the network on these paths.

use assert_cmd::Command;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const NAME: &str = "balls-plugin-github";

/// The ambient `GIT_*` vars the pre-commit hook exports; neither the binary
/// (which runs git against the project repo) nor the fixture `git` may inherit
/// them, or they would target the hook's repo instead of our tempdirs.
const GIT_VARS: [&str; 5] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"];

fn bin() -> Command {
    let mut c = Command::cargo_bin(NAME).unwrap();
    for var in GIT_VARS {
        c.env_remove(var);
    }
    c
}

fn git(cwd: &Path, args: &[&str]) {
    let mut c = std::process::Command::new("git");
    c.arg("-C").arg(cwd);
    for var in GIT_VARS {
        c.env_remove(var);
    }
    assert!(c.args(args).output().unwrap().status.success());
}

/// A bare-minimum git repo on `main` (the project root the plugin pushes).
fn repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.name", "t"]);
    git(path, &["config", "user.email", "t@e"]);
    std::fs::write(path.join("README"), "x\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-qm", "init"]);
}

/// A change worktree with a staged `tasks/<id>.md` deletion (what `close` hands
/// the plugin on the pre wire, §7).
fn change_worktree(path: &Path, id: &str) {
    repo(path);
    std::fs::create_dir(path.join("tasks")).unwrap();
    std::fs::write(path.join("tasks").join(format!("{id}.md")), "+++\n+++\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-qm", "task"]);
    git(path, &["rm", "-q", &format!("tasks/{id}.md")]);
}

fn fake_bl(dir: &Path) -> PathBuf {
    let p = dir.join("bl");
    std::fs::write(&p, "#!/bin/sh\ncase \"$1\" in create) echo bl-gate;; esac\nexit 0\n").unwrap();
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

fn write_config(landing: &Path, target: Option<&str>) {
    let dir = landing.join("config/plugins");
    std::fs::create_dir_all(&dir).unwrap();
    let t = target.map(|s| format!(r#","target_branch":"{s}""#)).unwrap_or_default();
    std::fs::write(dir.join(format!("{NAME}.json")), format!(r#"{{"repo":"o/n","api_base":"http://127.0.0.1:1"{t}}}"#)).unwrap();
}

fn write_token(state: &Path) {
    let dir = state.join(format!("balls/plugins/{NAME}/auth"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token.json"), r#"{"api_base":"http://127.0.0.1:1","token":"t"}"#).unwrap();
}

fn gate_file(state: &Path, invocation: &str, parent: &str) -> PathBuf {
    state
        .join(format!("balls/plugins/{NAME}/by-project"))
        .join(invocation.trim_start_matches('/'))
        .join("gates")
        .join(parent)
}

/// A hook invocation pre-wired with config + token + fake bl + env.
fn hook(tmp: &Path, op: &str, phase: &str, wire: &str) -> Command {
    write_config(&tmp.join("landing"), Some("main"));
    write_token(&tmp.join("state"));
    let mut c = bin();
    c.args([op, phase])
        .env("XDG_STATE_HOME", tmp.join("state"))
        .env("BALLS_PLUGIN_NAME", NAME)
        .env("BALLS_BL", fake_bl(tmp))
        .write_stdin(wire.to_string());
    c
}

#[test]
fn protocol_self_describes() {
    // env stripped so the read_env fallbacks (no XDG/BL/NAME) are exercised
    bin()
        .arg("protocol")
        .env_remove("XDG_STATE_HOME")
        .env_remove("BALLS_BL")
        .env_remove("BALLS_PLUGIN_NAME")
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""ops":["claim","close","drop","sync"]"#));
}

#[test]
fn no_args_or_missing_phase_is_usage_error() {
    bin().assert().failure();
    bin().arg("claim").assert().failure();
}

#[test]
fn bad_wire_is_an_error() {
    bin()
        .args(["claim", "post"])
        .env("BALLS_PLUGIN_NAME", NAME)
        .write_stdin("not json")
        .assert()
        .failure()
        .stderr(predicates::str::contains("wire"));
}

#[test]
fn claim_post_opens_and_remembers_the_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    repo(&proj);
    let inv = proj.to_string_lossy().into_owned();
    let wire = format!(
        r#"{{"actor":"alice","binding":{{"invocation_path":"{inv}","landing":"{}/landing"}},
            "metadata":{{"bl-id":["bl-1"]}},"current_state":{{"title":"Do it"}}}}"#,
        tmp.path().to_string_lossy()
    );
    hook(tmp.path(), "claim", "post", &wire).assert().success();
    let gate = gate_file(&tmp.path().join("state"), &inv, "bl-1");
    assert_eq!(std::fs::read_to_string(gate).unwrap(), "bl-gate");
}

#[test]
fn close_pre_empty_deliverable_auto_resolves_the_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    repo(&proj); // no work/bl-1 branch -> empty deliverable
    let cwd = tmp.path().join("change");
    change_worktree(&cwd, "bl-1"); // resolve_id reads the staged deletion here
    let inv = proj.to_string_lossy().into_owned();
    let gate = gate_file(&tmp.path().join("state"), &inv, "bl-1");
    std::fs::create_dir_all(gate.parent().unwrap()).unwrap();
    std::fs::write(&gate, "bl-gate").unwrap();

    let wire = format!(
        r#"{{"binding":{{"invocation_path":"{inv}","landing":"{}/landing"}},"current_state":{{"title":"t"}}}}"#,
        tmp.path().to_string_lossy()
    );
    hook(tmp.path(), "close", "pre", &wire).current_dir(&cwd).assert().success();
    assert!(!gate.exists(), "gate should be forgotten after auto-resolve");
}

#[test]
fn sync_post_with_no_pending_gates_is_a_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let wire = format!(
        r#"{{"binding":{{"invocation_path":"{0}/proj","landing":"{0}/landing"}}}}"#,
        tmp.path().to_string_lossy()
    );
    hook(tmp.path(), "sync", "post", &wire).assert().success();
}

#[test]
fn missing_config_or_token_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let wire = format!(
        r#"{{"binding":{{"invocation_path":"/x","landing":"{}/landing"}},"metadata":{{"bl-id":["bl-1"]}}}}"#,
        tmp.path().to_string_lossy()
    );
    // no config written
    bin().args(["claim", "post"]).env("XDG_STATE_HOME", tmp.path().join("state")).env("BALLS_PLUGIN_NAME", NAME)
        .write_stdin(wire.clone()).assert().failure();
    // config present, token absent (fresh state dir)
    write_config(&tmp.path().join("landing"), Some("main"));
    bin().args(["claim", "post"]).env("XDG_STATE_HOME", tmp.path().join("state2")).env("BALLS_PLUGIN_NAME", NAME)
        .write_stdin(wire).assert().failure();
}

#[test]
fn close_without_a_target_branch_errors() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(&tmp.path().join("landing"), None); // no target_branch
    write_token(&tmp.path().join("state"));
    let wire = format!(
        r#"{{"binding":{{"invocation_path":"/x","landing":"{}/landing"}},"metadata":{{"bl-id":["bl-1"]}},"current_state":{{"title":"t"}}}}"#,
        tmp.path().to_string_lossy()
    );
    bin().args(["close", "pre"]).env("XDG_STATE_HOME", tmp.path().join("state")).env("BALLS_PLUGIN_NAME", NAME)
        .env("BALLS_BL", fake_bl(tmp.path())).write_stdin(wire).assert().failure()
        .stderr(predicates::str::contains("target_branch"));
}

#[test]
fn auth_setup_then_check_round_trip() {
    let mut server = mockito::Server::new();
    server.mock("GET", "/user").with_status(200).with_body(r#"{"login":"octocat"}"#).expect_at_least(2).create();
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");

    bin().args(["auth-setup", &server.url()]).env("XDG_STATE_HOME", &state).env("BALLS_PLUGIN_NAME", NAME)
        .write_stdin("ghp_tok\n").assert().success().stdout(predicates::str::contains("octocat"));
    bin().args(["auth-check", &server.url()]).env("XDG_STATE_HOME", &state).env("BALLS_PLUGIN_NAME", NAME)
        .assert().success().stdout(predicates::str::contains("octocat"));
}
