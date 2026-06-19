//! End-to-end tests: run the actual binary so `main` and the `edge` dispatch are
//! exercised (and counted by coverage). The forge plugin resolves `bl` on
//! `$PATH` (core sets no BALLS_BL; §6/§7), so the fake `bl` is injected by
//! prepending its directory to PATH; only the merged-PR probe touches HTTP
//! (mockito).

use assert_cmd::Command;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const NAME: &str = "balls-plugin-github";

/// The ambient `GIT_*` vars the pre-commit hook exports; the binary's nested
/// `bl` strips them itself, but the test harness must not leak them either.
const GIT_VARS: [&str; 5] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"];

fn bin() -> Command {
    let mut c = Command::cargo_bin(NAME).unwrap();
    for var in GIT_VARS {
        c.env_remove(var);
    }
    c
}

/// A fake `bl` in the (per-test) project dir: logs argv to `bl.log`, mints an
/// id on `create`, replays `list.json` on `list` (empty list if absent).
fn fake_bl(proj: &Path) -> PathBuf {
    let p = proj.join("bl");
    let script = "#!/bin/sh\necho \"$@\" >> bl.log\ncase \"$1\" in\n\
         create) echo bl-gate;;\n\
         list) [ -f list.json ] && cat list.json || echo '[]';;\n\
         esac\nexit 0\n";
    std::fs::write(&p, script).unwrap();
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

fn write_config(landing: &Path, api_base: &str) {
    let dir = landing.join(format!("config/plugins/{NAME}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.json"), format!(r#"{{"repo":"o/n","api_base":"{api_base}"}}"#))
        .unwrap();
}

fn write_token(state: &Path, api_base: &str) {
    let dir = state.join(format!("balls/plugins/{NAME}/auth"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token.json"), format!(r#"{{"api_base":"{api_base}","token":"t"}}"#)).unwrap();
}

/// The dead-end loopback base for paths that must never reach the network.
const NO_API: &str = "http://127.0.0.1:1";

/// A hook invocation pre-wired with config + token + fake bl + env. The project
/// dir (= `binding.invocation_path`) is `<tmp>/proj`; `bl.log` lands there.
fn hook(tmp: &Path, op: &str, phase: &str, wire: &str, api_base: &str) -> Command {
    let proj = tmp.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    write_config(&tmp.join("landing"), api_base);
    write_token(&tmp.join("state"), api_base);
    // The fake `bl` lives in `proj`; the plugin resolves `bl` on $PATH, so
    // prepend that directory (core injects no BALLS_BL — §6/§7).
    let bl_dir = fake_bl(&proj).parent().unwrap().to_path_buf();
    let mut c = bin();
    c.args([op, phase])
        .env("XDG_STATE_HOME", tmp.join("state"))
        .env("BALLS_PLUGIN_NAME", NAME)
        .env("PATH", with_path_prefix(&bl_dir))
        .write_stdin(wire.to_string());
    c
}

/// Prepend `dir` to the inherited `$PATH` so the fake `bl` shadows any real one.
fn with_path_prefix(dir: &Path) -> String {
    match std::env::var("PATH") {
        Ok(p) => format!("{}:{p}", dir.display()),
        Err(_) => dir.display().to_string(),
    }
}

/// A claim.post wire for `<tmp>/proj`, with `extra_state` spliced into
/// `previous_state` after the title.
fn claim_wire(tmp: &Path, extra_state: &str) -> String {
    format!(
        r#"{{"actor":"alice","binding":{{"invocation_path":"{proj}","landing":"{land}"}},
            "metadata":{{"bl-id":["bl-1"]}},"previous_state":{{"title":"Do it"{extra_state}}}}}"#,
        proj = tmp.join("proj").display(),
        land = tmp.join("landing").display(),
    )
}

fn bl_log(tmp: &Path) -> String {
    std::fs::read_to_string(tmp.join("proj/bl.log")).unwrap_or_default()
}

#[test]
fn protocol_self_describes() {
    // env stripped so the read_env fallbacks (no XDG/NAME) are exercised
    bin()
        .arg("protocol")
        .env_remove("XDG_STATE_HOME")
        .env_remove("BALLS_PLUGIN_NAME")
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""ops":["claim","sync"]"#));
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
fn claim_post_mints_the_gate_child_and_prints_its_id() {
    let tmp = tempfile::tempdir().unwrap();
    let wire = claim_wire(tmp.path(), "");
    hook(tmp.path(), "claim", "post", &wire, NO_API)
        .assert()
        .success()
        .stdout(predicates::str::contains("bl-gate"));
    let log = bl_log(tmp.path());
    assert!(log.contains("list --json"), "{log}"); // the standing-gate check
    assert!(log.contains("create --parent bl-1 --blocks close --as alice -- Review gate: Do it"), "{log}");
    assert!(log.contains(&format!("update bl-gate {NAME}=bl-1 --as alice")), "{log}");
}

#[test]
fn claim_post_on_a_gate_child_mints_nothing() {
    // The claimed ball carries the plugin's own join key — no gates-for-gates.
    let tmp = tempfile::tempdir().unwrap();
    let wire = claim_wire(tmp.path(), &format!(r#","{NAME}":"bl-elder""#));
    hook(tmp.path(), "claim", "post", &wire, NO_API).assert().success().stdout(predicates::str::is_empty());
    assert!(!bl_log(tmp.path()).contains("create"));
}

#[test]
fn claim_post_reuses_a_standing_gate() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("proj")).unwrap();
    std::fs::write(
        tmp.path().join("proj/list.json"),
        format!(r#"[{{"id":"bl-g1","{NAME}":"bl-1"}}]"#),
    )
    .unwrap();
    let wire = claim_wire(tmp.path(), "");
    hook(tmp.path(), "claim", "post", &wire, NO_API).assert().success().stdout(predicates::str::is_empty());
    assert!(!bl_log(tmp.path()).contains("create"));
}

#[test]
fn rollback_claim_post_closes_the_minted_gate() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("proj")).unwrap();
    std::fs::write(
        tmp.path().join("proj/list.json"),
        format!(r#"[{{"id":"bl-g1","{NAME}":"bl-1"}}]"#),
    )
    .unwrap();
    let wire = format!(
        r#"{{"actor":"alice","binding":{{"invocation_path":"{proj}","landing":"{land}"}},
            "metadata":{{"bl-id":["bl-1"]}},"rolling_back":"post"}}"#,
        proj = tmp.path().join("proj").display(),
        land = tmp.path().join("landing").display(),
    );
    hook(tmp.path(), "claim", "post", &wire, NO_API).assert().success();
    let log = bl_log(tmp.path());
    assert!(log.contains("close bl-g1 -m review gate withdrawn"), "{log}");
}

#[test]
fn sync_post_closes_the_gate_once_the_pr_merges() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .match_query(mockito::Matcher::UrlEncoded("head".into(), "o:work/bl-1".into()))
        .with_status(200)
        .with_body(r#"[{"html_url":"https://gh/pr/4","merged_at":"2026-06-09T00:00:00Z"}]"#)
        .create();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("proj")).unwrap();
    std::fs::write(
        tmp.path().join("proj/list.json"),
        format!(r#"[{{"id":"bl-g1","{NAME}":"bl-1"}}]"#),
    )
    .unwrap();
    let wire = format!(
        r#"{{"actor":"alice","binding":{{"invocation_path":"{proj}","landing":"{land}"}}}}"#,
        proj = tmp.path().join("proj").display(),
        land = tmp.path().join("landing").display(),
    );
    hook(tmp.path(), "sync", "post", &wire, &s.url())
        .assert()
        .success()
        .stdout(predicates::str::contains("bl-g1 resolved: bl-1 merged (https://gh/pr/4)"));
    assert!(bl_log(tmp.path()).contains("close bl-g1 -m PR merged: https://gh/pr/4"));
}

#[test]
fn sync_post_leaves_unmerged_gates_open() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body("[]")
        .create();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("proj")).unwrap();
    std::fs::write(
        tmp.path().join("proj/list.json"),
        format!(r#"[{{"id":"bl-g1","{NAME}":"bl-1"}}]"#),
    )
    .unwrap();
    let wire = format!(
        r#"{{"binding":{{"invocation_path":"{proj}","landing":"{land}"}}}}"#,
        proj = tmp.path().join("proj").display(),
        land = tmp.path().join("landing").display(),
    );
    hook(tmp.path(), "sync", "post", &wire, &s.url()).assert().success().stdout(predicates::str::is_empty());
    assert!(!bl_log(tmp.path()).contains("close"));
}

#[test]
fn hook_errors_cleanly_without_config_or_token() {
    let tmp = tempfile::tempdir().unwrap();
    let wire = format!(
        r#"{{"binding":{{"invocation_path":"/x","landing":"{}/landing"}},"metadata":{{"bl-id":["bl-1"]}}}}"#,
        tmp.path().display()
    );
    // no config written
    bin().args(["claim", "post"]).env("XDG_STATE_HOME", tmp.path().join("state")).env("BALLS_PLUGIN_NAME", NAME)
        .write_stdin(wire.clone()).assert().failure();
    // config present, token absent (fresh state dir)
    write_config(&tmp.path().join("landing"), NO_API);
    bin().args(["claim", "post"]).env("XDG_STATE_HOME", tmp.path().join("state2")).env("BALLS_PLUGIN_NAME", NAME)
        .write_stdin(wire).assert().failure();
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
