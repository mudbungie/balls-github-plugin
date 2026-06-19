use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const UA_KEY: &str = "balls-plugin-github";

/// A fake `bl` logging argv to `bl.log`; `create` mints an id, `list` replays
/// `list.json` from cwd, `update` exits with the contents of `update.exit` (if
/// present).
fn fake_bl(dir: &Path) -> PathBuf {
    let p = dir.join("bl");
    let script = "#!/bin/sh\necho \"$@\" >> bl.log\ncase \"$1\" in\n\
         create) echo bl-gate;;\n\
         list) cat list.json;;\n\
         update) [ -f update.exit ] && exit \"$(cat update.exit)\";;\n\
         esac\nexit 0\n";
    std::fs::write(&p, script).unwrap();
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

fn project(dir: &Path, api_base: &str) -> Project {
    let config: RepoConfig = serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":{api_base:?}}}"#)).unwrap();
    let bl = Bl::new(&fake_bl(dir), dir, "alice");
    Project::new(&config, "tok", UA_KEY.into(), bl)
}

fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("bl.log")).unwrap_or_default()
}

/// The dead-end loopback base for paths that must never reach the network.
const NO_API: &str = "http://127.0.0.1:1";

#[test]
fn open_gates_scans_the_live_list() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("list.json"),
        r#"[{"id":"bl-g","balls-plugin-github":"bl-p"},{"id":"bl-w"}]"#,
    )
    .unwrap();
    let p = project(dir.path(), NO_API);
    let gates = p.open_gates().unwrap();
    assert_eq!(gates, vec![Gate { id: "bl-g".into(), parent: "bl-p".into() }]);
    assert!(log(dir.path()).contains("list --json"));
}

#[test]
fn mint_gate_creates_then_stamps_the_join_key() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(dir.path(), NO_API);
    assert_eq!(p.mint_gate("bl-p", "Do it").unwrap(), "bl-gate");
    let log = log(dir.path());
    assert!(log.contains("create --parent bl-p --blocks close --as alice -- Review gate: Do it"), "{log}");
    assert!(log.contains("update bl-gate balls-plugin-github=bl-p --as alice"), "{log}");
}

#[test]
fn mint_gate_withdraws_the_half_minted_gate_when_the_stamp_fails() {
    // §14: a failing plugin cleans up inline — the keyless gate would be
    // underivable, so it is closed before the error surfaces.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("update.exit"), "1").unwrap();
    let p = project(dir.path(), NO_API);
    assert!(p.mint_gate("bl-p", "t").is_err());
    assert!(log(dir.path()).contains("close bl-gate -m withdrawn"), "{}", log(dir.path()));
}

#[test]
fn close_gate_delegates_to_bl_close() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(dir.path(), NO_API);
    p.close_gate("bl-g", "PR merged: u").unwrap();
    assert!(log(dir.path()).contains("close bl-g -m PR merged: u --as alice"));
}

#[test]
fn merged_pr_returns_the_url_only_once_merged() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .match_query(mockito::Matcher::UrlEncoded("head".into(), "o:work/bl-p".into()))
        .with_status(200)
        .with_body(r#"[{"html_url":"https://gh/pr/4","merged_at":"2026-06-09T00:00:00Z"}]"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let p = project(dir.path(), &s.url());
    assert_eq!(p.merged_pr("bl-p").unwrap(), Some("https://gh/pr/4".into()));
}

#[test]
fn merged_pr_is_none_for_an_open_pr_or_no_pr() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body(r#"[{"html_url":"u","merged_at":null}]"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(project(dir.path(), &s.url()).merged_pr("bl-p").unwrap(), None);

    let mut s2 = mockito::Server::new();
    s2.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body("[]")
        .create();
    let dir2 = tempfile::tempdir().unwrap();
    assert_eq!(project(dir2.path(), &s2.url()).merged_pr("bl-p").unwrap(), None);
}
