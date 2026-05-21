//! Unit tests for `commands::push`. Lifted out of `push.rs` to keep
//! that file under the 300-line workspace cap (mirrors the
//! `pull_emit.rs` / `pull_emit_tests.rs` pattern).

use super::*;

const UA: &str = "balls-plugin-github-issues-test";

fn cfg(api: &str) -> PluginConfig {
    serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":{:?}}}"#, api)).unwrap()
}

fn task(json: &str) -> Task {
    serde_json::from_str(json).unwrap()
}

#[test]
fn noop_when_closed_without_stored_number() {
    let c = GithubClient::new("http://x", "t", UA);
    let v = push_task(
        &c,
        &cfg("http://x"),
        &task(r#"{"id":"bl-1","title":"t","status":"closed"}"#),
    )
    .unwrap();
    assert_eq!(v, json!({}));
}

#[test]
fn noop_when_status_title_and_body_all_match_last_sync() {
    let c = GithubClient::new("http://x", "t", UA);
    // body defaults to "" so the expected hash is body_hash("").
    let empty = body_hash("");
    let v = push_task(
        &c,
        &cfg("http://x"),
        &task(&format!(
            r#"{{"id":"bl-2","title":"t","status":"open",
                "external":{{"github-issues":{{"issue":{{
                    "number":3,"url":"u","state":"open",
                    "source":"balls","synced_at":"t",
                    "last_synced_status":"open",
                    "last_synced_title":"t [bl-2]",
                    "last_synced_body_hash":"{empty}"}}}}}}}}"#
        )),
    )
    .unwrap();
    assert_eq!(v, json!({}));
}

#[test]
fn patches_when_only_title_changed_since_last_sync() {
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/8")
        .with_status(200)
        .with_body(r#"{"number":8,"html_url":"u","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let empty = body_hash("");
    // task.title is "New", but last_synced_title is the prior pushed
    // title — status and body are unchanged. The mirror must PATCH and
    // update the projection.
    let v = push_task(
        &c,
        &cfg(&s.url()),
        &task(&format!(
            r#"{{"id":"bl-7","title":"New","status":"open",
                "external":{{"github-issues":{{"issue":{{
                    "number":8,"url":"u","state":"open",
                    "source":"balls","synced_at":"t",
                    "last_synced_status":"open",
                    "last_synced_title":"Old [bl-7]",
                    "last_synced_body_hash":"{empty}"}}}}}}}}"#
        )),
    )
    .unwrap();
    assert_eq!(v["issue"]["last_synced_title"], "New [bl-7]");
    assert_eq!(v["issue"]["last_synced_status"], "open");
}

#[test]
fn patches_when_only_body_changed_since_last_sync() {
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/9")
        .with_status(200)
        .with_body(r#"{"number":9,"html_url":"u","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    // Status and title match; only description hash moved.
    let v = push_task(
        &c,
        &cfg(&s.url()),
        &task(
            r#"{"id":"bl-9","title":"t","status":"open","description":"new body",
                "external":{"github-issues":{"issue":{
                    "number":9,"url":"u","state":"open",
                    "source":"balls","synced_at":"t",
                    "last_synced_status":"open",
                    "last_synced_title":"t [bl-9]",
                    "last_synced_body_hash":"0000000000000000"}}}}"#,
        ),
    )
    .unwrap();
    assert_eq!(v["issue"]["last_synced_body_hash"], body_hash("new body"));
}

#[test]
fn rejects_bad_repo() {
    let c = GithubClient::new("http://x", "t", UA);
    let conf: PluginConfig = serde_json::from_str(r#"{"repo":"noslash"}"#).unwrap();
    assert!(push_task(
        &c,
        &conf,
        &task(r#"{"id":"bl-1","title":"t","status":"open"}"#),
    )
    .is_err());
}

#[test]
fn creates_issue_when_no_stored_number() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/repos/o/n/issues")
        .with_status(201)
        .with_body(r#"{"number":7,"html_url":"https://gh/i/7","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let v = push_task(
        &c,
        &cfg(&s.url()),
        &task(r#"{"id":"bl-3","title":"Do it","status":"open","description":"body"}"#),
    )
    .unwrap();
    let issue = &v["issue"];
    assert_eq!(issue["number"], 7);
    assert_eq!(issue["source"], "balls");
    assert_eq!(issue["last_synced_status"], "open");
    // bl-4918: push records what it just sent to GH so the next
    // sync's content-mirror can ask "who moved?".
    assert_eq!(issue["last_synced_title"], "Do it [bl-3]");
    assert_eq!(issue["last_synced_body_hash"], body_hash("body"));
}

#[test]
fn patches_existing_issue_on_status_change_close() {
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/4")
        .with_status(200)
        .with_body(r#"{"number":4,"html_url":"u","state":"closed"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let v = push_task(
        &c,
        &cfg(&s.url()),
        &task(
            r#"{"id":"bl-4","title":"t","status":"closed",
                "external":{"github-issues":{"issue":{
                    "number":4,"url":"u","state":"open",
                    "source":"balls","synced_at":"t",
                    "last_synced_status":"open"}}}}"#,
        ),
    )
    .unwrap();
    let issue = &v["issue"];
    assert_eq!(issue["state"], "closed");
    assert_eq!(issue["last_synced_status"], "closed");
}

#[test]
fn patches_existing_issue_on_status_change_reopen() {
    // open -> in_progress is a status change even though both map to
    // GH state=open; we still PATCH so the title/body mirror moves
    // and the last_synced_status updates.
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/5")
        .with_status(200)
        .with_body(r#"{"number":5,"html_url":"u","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let v = push_task(
        &c,
        &cfg(&s.url()),
        &task(
            r#"{"id":"bl-5","title":"t","status":"in_progress",
                "external":{"github-issues":{"issue":{
                    "number":5,"url":"u","state":"open",
                    "source":"balls","synced_at":"t",
                    "last_synced_status":"open"}}}}"#,
        ),
    )
    .unwrap();
    assert_eq!(v["issue"]["last_synced_status"], "in_progress");
}
