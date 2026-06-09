//! Tests for the pull (sync) reconcile. A fixture wires a mockito GitHub
//! server, a fake `bl` (records argv, prints a fixed id for `create`), a temp
//! store, and a temp territory.

use super::*;
use crate::config::{CloseMirror, OnExternalDelete};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const UA: &str = "balls-plugin-github-issues-test";
const NEW_ID: &str = "bl-abcd";

struct Fixture {
    server: mockito::ServerGuard,
    store: PathBuf,
    territory: PathBuf,
    bl_log: PathBuf,
    bl_bin: OsString,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir_all(store.join("tasks")).unwrap();
        let territory = dir.path().join("territory");
        let bl_log = dir.path().join("bl.log");
        let bl_bin = dir.path().join("bl");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> {log}\nif [ \"$1\" = create ]; then echo 'create {id}'; fi\nexit 0\n",
            log = bl_log.display(),
            id = NEW_ID,
        );
        std::fs::write(&bl_bin, script).unwrap();
        std::fs::set_permissions(&bl_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { server: mockito::Server::new(), store, territory, bl_log, bl_bin: bl_bin.into(), _dir: dir }
    }

    fn write_ball(&self, id: &str, title: &str, body: &str) {
        let md = format!("+++\ntitle = \"{title}\"\ncreated = 1\nupdated = 1\n+++\n{body}");
        std::fs::write(self.store.join("tasks").join(format!("{id}.md")), md).unwrap();
    }

    fn cfg(&self, delete: OnExternalDelete, close: CloseMirror, label: Option<&str>) -> PluginConfig {
        serde_json::from_str(&format!(
            r#"{{"repo":"o/n","api_base":"{}","on_external_delete":"{}","close_mirror":"{}"{}}}"#,
            self.server.url(),
            match delete {
                OnExternalDelete::Deferred => "deferred",
                OnExternalDelete::Closed => "closed",
                OnExternalDelete::Noop => "noop",
            },
            match close {
                CloseMirror::Authoritative => "authoritative",
                CloseMirror::BestEffort => "best_effort",
                CloseMirror::Off => "off",
            },
            label.map_or(String::new(), |l| format!(r#","target_label":"{l}""#)),
        ))
        .unwrap()
    }

    fn bl(&self) -> Bl {
        Bl::new(self.bl_bin.clone(), self.store.clone(), "tester".into())
    }

    fn run(&mut self, cfg: &PluginConfig, base: &mut Base) -> Result<()> {
        let client = GithubClient::new(cfg.api_base(), "tok", UA);
        let bl = self.bl();
        pull(&client, "o", "n", cfg, base, &self.store, &self.territory, &bl)
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.bl_log).unwrap_or_default()
    }
}

fn list_mock(f: &mut Fixture, body: &str) -> mockito::Mock {
    f.server
        .mock("GET", "/repos/o/n/issues?state=all&per_page=100")
        .with_status(200)
        .with_body(body)
        .create()
}

fn snap(number: u64, title: &str, body: &str, state: &str) -> Snapshot {
    Snapshot { number, title: title.into(), body_hash: body_hash(body), state: state.into() }
}

#[test]
fn autocreate_imports_open_unmarked_issue() {
    let mut f = Fixture::new();
    let list = list_mock(&mut f, r#"[{"number":5,"title":"External","state":"open","body":"rep"}]"#);
    let stamp = f
        .server
        .mock("PATCH", "/repos/o/n/issues/5")
        .match_body(r#"{"title":"External [bl-abcd]"}"#)
        .with_status(200)
        .with_body(r#"{"number":5,"state":"open"}"#)
        .create();
    let mut base = Base::default();
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None), &mut base).unwrap();
    list.assert();
    stamp.assert();
    assert!(f.calls().contains("create --body rep --as tester -- External"), "{}", f.calls());
    let s = base.get(NEW_ID).unwrap();
    assert_eq!(s.number, 5);
    assert_eq!(s.title, "External");
}

#[test]
fn linked_issue_reasserts_content_when_drifted() {
    let mut f = Fixture::new();
    f.write_ball("bl-1a2b", "Real title", "real body");
    list_mock(&mut f, r#"[{"number":9,"title":"Drifted [bl-1a2b]","state":"open","body":"old"}]"#);
    let patch = f
        .server
        .mock("PATCH", "/repos/o/n/issues/9")
        .match_body(r#"{"body":"real body","title":"Real title [bl-1a2b]"}"#)
        .with_status(200)
        .with_body(r#"{"number":9,"state":"open"}"#)
        .create();
    let mut base = Base::default();
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None), &mut base).unwrap();
    patch.assert();
    assert_eq!(base.get("bl-1a2b").unwrap().title, "Real title");
}

#[test]
fn linked_issue_converged_makes_no_patch() {
    let mut f = Fixture::new();
    f.write_ball("bl-1a2b", "Same", "body");
    list_mock(&mut f, r#"[{"number":9,"title":"Same [bl-1a2b]","state":"open","body":"body"}]"#);
    // No PATCH mock: a stray PATCH would 501 and error the run.
    let mut base = Base::default();
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None), &mut base).unwrap();
    assert_eq!(base.get("bl-1a2b").unwrap().number, 9);
}

#[test]
fn close_mirror_closes_live_task() {
    let mut f = Fixture::new();
    f.write_ball("bl-1a2b", "T", "b");
    list_mock(&mut f, r#"[{"number":9,"title":"T [bl-1a2b]","state":"closed","body":"b"}]"#);
    let mut base = Base::default();
    base.set("bl-1a2b", snap(9, "T", "b", "open"));
    // best_effort also mirrors the close (only `off` opts out).
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::BestEffort, None), &mut base).unwrap();
    assert!(f.calls().contains("close bl-1a2b"));
    assert!(base.get("bl-1a2b").is_none());
}

#[test]
fn close_mirror_off_reasserts_instead() {
    let mut f = Fixture::new();
    f.write_ball("bl-1a2b", "T", "b");
    // Closed on GH but close_mirror=off and content matches → no close, no patch.
    list_mock(&mut f, r#"[{"number":9,"title":"T [bl-1a2b]","state":"closed","body":"b"}]"#);
    let mut base = Base::default();
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Off, None), &mut base).unwrap();
    assert!(!f.calls().contains("close"));
    assert_eq!(base.get("bl-1a2b").unwrap().state, "closed");
}

#[test]
fn linked_task_gone_catches_up_close() {
    let mut f = Fixture::new();
    // ball file absent; issue still open and marked.
    list_mock(&mut f, r#"[{"number":9,"title":"Ghost [bl-dead]","state":"open","body":""}]"#);
    let close = f
        .server
        .mock("PATCH", "/repos/o/n/issues/9")
        .match_body(r#"{"state":"closed"}"#)
        .with_status(200)
        .with_body(r#"{"number":9,"state":"closed"}"#)
        .create();
    let mut base = Base::default();
    base.set("bl-dead", snap(9, "Ghost", "", "open"));
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None), &mut base).unwrap();
    close.assert();
    assert!(base.get("bl-dead").is_none());
}

#[test]
fn sweep_defers_then_closes_vanished_issues() {
    let mut f = Fixture::new();
    f.write_ball("bl-gone", "G", "b");
    list_mock(&mut f, "[]"); // nothing on GH → every base entry vanished
    let mut base = Base::default();
    base.set("bl-gone", snap(3, "G", "b", "open"));
    base.set("bl-dead", snap(4, "D", "b", "open")); // no ball → forgotten, no verb
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None), &mut base).unwrap();
    assert!(f.calls().contains("update bl-gone -t deferred"));
    assert!(base.get("bl-dead").is_none()); // stale link dropped

    // Closed policy on the same vanished-but-live ball.
    let mut f2 = Fixture::new();
    f2.write_ball("bl-gone", "G", "b");
    list_mock(&mut f2, "[]");
    let mut base2 = Base::default();
    base2.set("bl-gone", snap(3, "G", "b", "open"));
    f2.run(&f2.cfg(OnExternalDelete::Closed, CloseMirror::Authoritative, None), &mut base2).unwrap();
    assert!(f2.calls().contains("close bl-gone"));
    assert!(base2.get("bl-gone").is_none());
}

#[test]
fn sweep_noop_policy_leaves_task() {
    let mut f = Fixture::new();
    f.write_ball("bl-gone", "G", "b");
    list_mock(&mut f, "[]");
    let mut base = Base::default();
    base.set("bl-gone", snap(3, "G", "b", "open"));
    f.run(&f.cfg(OnExternalDelete::Noop, CloseMirror::Authoritative, None), &mut base).unwrap();
    assert!(f.calls().is_empty()); // no verb shelled
    assert!(base.get("bl-gone").is_some());
}

#[test]
fn target_label_filters_and_closed_issues_are_not_imported() {
    let mut f = Fixture::new();
    list_mock(
        &mut f,
        r#"[{"number":1,"title":"unlabelled","state":"open","body":""},
            {"number":2,"title":"done","state":"closed","body":"","labels":[{"name":"track"}]}]"#,
    );
    let mut base = Base::default();
    f.run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, Some("track")), &mut base)
        .unwrap();
    // #1 filtered (no label); #2 in scope but closed → not auto-created.
    assert!(f.calls().is_empty());
    assert!(base.entries.is_empty());
}

#[test]
fn list_error_aborts_the_sync() {
    let mut f = Fixture::new();
    f.server
        .mock("GET", "/repos/o/n/issues?state=all&per_page=100")
        .with_status(500)
        .create();
    let mut base = Base::default();
    assert!(f
        .run(&f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None), &mut base)
        .is_err());
}

#[test]
fn autocreate_cap_blocks_further_imports() {
    // Drive autocreate directly with the counter already at the cap.
    let f = Fixture::new();
    let client = GithubClient::new("http://127.0.0.1:1", "t", UA); // unreachable: must not be called
    let bl = f.bl();
    let issue: GhIssue =
        serde_json::from_str(r#"{"number":1,"title":"x","state":"open","body":""}"#).unwrap();
    let mut base = Base::default();
    let mut created = MAX_AUTOCREATES;
    autocreate(
        "x",
        &issue,
        &f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, None),
        &mut base,
        &bl,
        &client,
        "o",
        "n",
        &mut created,
    )
    .unwrap();
    assert!(base.entries.is_empty());
    assert!(f.calls().is_empty());
}

#[test]
fn truncate_body_caps_and_marks() {
    let short = "small";
    assert_eq!(truncate_body(short), short);
    let big = "x".repeat(MAX_BODY_BYTES * 2);
    let cut = truncate_body(&big);
    assert!(cut.len() < big.len());
    assert!(cut.contains("truncated by ingest defense"));
    // A multibyte char straddling the cap forces the char-boundary backoff.
    let multibyte = format!("{}{}", "x".repeat(MAX_BODY_BYTES - 1), "é".repeat(20));
    assert!(truncate_body(&multibyte).contains("truncated"));
}

#[test]
fn labels_of_drops_target_and_caps() {
    let f = Fixture::new();
    let issue: GhIssue = serde_json::from_str(
        r#"{"number":1,"title":"x","state":"open","labels":[{"name":"bug"},{"name":"track"}]}"#,
    )
    .unwrap();
    let cfg = f.cfg(OnExternalDelete::Deferred, CloseMirror::Authoritative, Some("track"));
    assert_eq!(labels_of(&issue, &cfg), vec!["bug".to_string()]);
}
