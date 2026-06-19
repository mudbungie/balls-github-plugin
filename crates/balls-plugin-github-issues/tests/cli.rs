//! Integration coverage for the binary edge (`main.rs`): the §6 `protocol`
//! self-describe, the usage-error exit path, the env wiring (the `bl` resolved
//! on `$PATH`, the import guard, `XDG_STATE_HOME`), and one full pull through the
//! built binary against a throwaway store + mock GitHub.

use assert_cmd::Command;
use predicates::str::contains;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn bin() -> Command {
    let mut c = Command::cargo_bin("balls-plugin-github-issues").unwrap();
    c.env_remove("BALLS_GITHUB_ISSUES_IMPORTING");
    c
}

/// A fake `bl` recording argv + the import-guard env to `<dir>/bl.log` and
/// printing a minted id for `create`.
fn fake_bl(dir: &Path) -> PathBuf {
    let log = dir.join("bl.log");
    let p = dir.join("bl");
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> {log}\necho \"guard=$BALLS_GITHUB_ISSUES_IMPORTING\" >> {log}\nif [ \"$1\" = create ]; then echo 'create bl-abcd'; fi\nexit 0\n",
        log = log.display(),
    );
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Prepend `dir` to the inherited `$PATH` so the fake `bl` shadows any real one.
fn with_path_prefix(dir: &Path) -> String {
    match std::env::var("PATH") {
        Ok(p) => format!("{}:{p}", dir.display()),
        Err(_) => dir.display().to_string(),
    }
}

#[test]
fn protocol_self_describes_over_the_process_boundary() {
    bin()
        .arg("protocol")
        .assert()
        .success()
        .stdout(contains(r#""protocol":1"#))
        .stdout(contains(r#""sync""#));
}

#[test]
fn an_unrecognized_invocation_exits_nonzero() {
    bin().args(["too", "many", "args"]).assert().failure();
}

#[test]
fn a_hook_with_a_malformed_payload_exits_nonzero() {
    // `sync post` reads a §7 payload from stdin; garbage aborts the op (§6).
    bin().args(["sync", "post"]).write_stdin("not json").assert().failure();
}

#[test]
fn the_import_guard_suppresses_a_push_slot_at_the_process_edge() {
    // The same create.post invocation against a landing with NO config:
    // with the guard env set, main resolves `importing` and the handler
    // suppresses itself before any config/token work (exit 0); without it,
    // the missing config aborts the op (exit 1). The pair pins main.rs's
    // env wiring, not just the unit-level Env flag.
    let payload =
        r#"{"op":"create","phase":"post","binding":{"invocation_path":"/p","landing":"/nope"},"metadata":{"bl-id":["bl-1a2b"]}}"#;
    bin()
        .args(["create", "post"])
        .env("BALLS_GITHUB_ISSUES_IMPORTING", "1")
        .write_stdin(payload)
        .assert()
        .success();
    bin().args(["create", "post"]).write_stdin(payload).assert().failure();
}

#[test]
fn sync_imports_an_external_issue_and_stamps_the_marker() {
    // One full pull through the BUILT BINARY: an unmarked open GitHub issue
    // becomes `bl create` (title behind `--`, guard env set, actor stamped),
    // and the minted id is stamped back onto the issue title — the [bl-xxxx]
    // join SSOT round trip, at the process boundary.
    let tmp = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let list = server
        .mock("GET", "/repos/o/n/issues?state=all&per_page=100")
        .with_status(200)
        .with_body(r#"[{"number":5,"title":"External","state":"open","body":"rep"}]"#)
        .create();
    let stamp = server
        .mock("PATCH", "/repos/o/n/issues/5")
        .match_body(r#"{"title":"External [bl-abcd]"}"#)
        .with_status(200)
        .with_body(r#"{"number":5,"state":"open"}"#)
        .create();

    // landing config + store checkout + territory token, all throwaway
    let landing = tmp.path().join("landing");
    let cfg_dir = landing.join("config/plugins/github-issues");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("config.json"),
        format!(r#"{{"repo":"o/n","api_base":"{}"}}"#, server.url()),
    )
    .unwrap();
    let invocation = tmp.path().join("proj");
    let store = invocation.join("store");
    std::fs::create_dir_all(store.join("tasks")).unwrap();
    let state = tmp.path().join("state");
    let territory = state
        .join("balls/plugins/github-issues")
        .join(invocation.to_str().unwrap().trim_start_matches('/'));
    std::fs::create_dir_all(&territory).unwrap();
    std::fs::write(
        territory.join("token.json"),
        format!(r#"{{"api_base":"{}","token":"t"}}"#, server.url()),
    )
    .unwrap();

    let payload = format!(
        r#"{{"op":"sync","phase":"post","actor":"me",
            "binding":{{"landing":"{}","store":"{}","invocation_path":"{}"}}}}"#,
        landing.display(),
        store.display(),
        invocation.display(),
    );
    // The plugin resolves `bl` on $PATH (core sets no BALLS_BIN; §6/§7); the
    // fake `bl` lives in `tmp`, so prepend that directory to PATH.
    let bl_dir = fake_bl(tmp.path()).parent().unwrap().to_path_buf();
    bin()
        .args(["sync", "post"])
        .env("XDG_STATE_HOME", &state)
        .env("PATH", with_path_prefix(&bl_dir))
        .write_stdin(payload)
        .assert()
        .success();

    list.assert();
    stamp.assert();
    let log = std::fs::read_to_string(tmp.path().join("bl.log")).unwrap();
    assert!(log.contains("create --body rep --as me -- External"), "bl argv: {log}");
    assert!(log.contains("guard=1"), "pull-driven verb must carry the import guard: {log}");
}
