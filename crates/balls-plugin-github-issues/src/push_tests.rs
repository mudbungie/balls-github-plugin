//! Tests for the push (balls → GitHub) reconcile. The ball's title and body are
//! read from a store checkout (`tasks/<id>.md`), the same source `sync`/pull
//! reads — bl-68db's fix for the push/pull body-source split.

use super::*;

const UA: &str = "balls-plugin-github-issues-test";

fn payload(json: &str) -> Payload {
    serde_json::from_str(json).unwrap()
}

fn snap(number: u64, title: &str, body_hash: &str, state: &str) -> Snapshot {
    Snapshot { number, title: title.into(), body_hash: body_hash.into(), state: state.into() }
}

/// A store checkout holding one `tasks/<id>.md` ball — the source push reads
/// title and body from (the same one `sync`/pull reads), exactly as
/// `binding.store` would after the seal ff-merges the op's commit.
fn store_with(id: &str, title: &str, body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join(format!("{id}.md")),
        format!("+++\ntitle = {title:?}\ncreated = 1\n+++\n{body}"),
    )
    .unwrap();
    dir
}

#[test]
fn create_posts_the_store_body_not_the_op_delta() {
    // bl-68db: the payload carries no `command.body_change`/`current_state`;
    // the issue body MUST come from the store ball, not an empty op delta.
    let store = store_with("bl-1a2b", "Hello", "the body");
    let terr = tempfile::tempdir().unwrap();
    let mut s = mockito::Server::new();
    s.mock("POST", "/repos/o/n/issues")
        .match_body(r#"{"body":"the body","title":"Hello [bl-1a2b]"}"#)
        .with_status(201)
        .with_body(r#"{"number":7,"html_url":"u","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let mut base = Base::default();
    let p = payload(
        r#"{"op":"create","phase":"post","binding":{"invocation_path":"/p"},
            "metadata":{"bl-id":["bl-1a2b"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
    let snap = base.get("bl-1a2b").unwrap();
    assert_eq!(snap.number, 7);
    assert_eq!(snap.title, "Hello");
    assert_eq!(snap.body_hash, body_hash("the body"));
    assert_eq!(snap.state, "open");
    // persisted to territory
    assert_eq!(Base::load(terr.path()).unwrap().get("bl-1a2b").unwrap().number, 7);
}

#[test]
fn update_patches_when_title_moves() {
    let store = store_with("bl-1a2b", "New", "fresh body");
    let terr = tempfile::tempdir().unwrap();
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/7")
        .match_body(r#"{"body":"fresh body","state":"open","title":"New [bl-1a2b]"}"#)
        .with_status(200)
        .with_body(r#"{"number":7,"html_url":"u","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let mut base = Base::default();
    base.set("bl-1a2b", snap(7, "Old", "h", "open"));
    let p = payload(
        r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"},
            "metadata":{"bl-id":["bl-1a2b"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
    assert_eq!(base.get("bl-1a2b").unwrap().title, "New");
}

#[test]
fn update_patches_when_only_body_moves() {
    // The store body moved (title and state unchanged) — the PATCH carries the
    // new body. With the old `body_change`-delta source a title-touching op
    // would have shipped a stale body here.
    let store = store_with("bl-1a2b", "Same", "new body");
    let terr = tempfile::tempdir().unwrap();
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/7")
        .match_body(r#"{"body":"new body","state":"open","title":"Same [bl-1a2b]"}"#)
        .with_status(200)
        .with_body(r#"{"number":7,"html_url":"u","state":"open"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let mut base = Base::default();
    base.set("bl-1a2b", snap(7, "Same", body_hash("old body").as_str(), "open"));
    let p = payload(
        r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"},
            "metadata":{"bl-id":["bl-1a2b"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
    assert_eq!(base.get("bl-1a2b").unwrap().body_hash, body_hash("new body"));
}

#[test]
fn update_noops_when_nothing_moved() {
    let store = store_with("bl-1a2b", "Same", "b");
    let terr = tempfile::tempdir().unwrap();
    // No mock server hit: if push made any call it would error (no server).
    let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
    let mut base = Base::default();
    base.set("bl-1a2b", snap(7, "Same", body_hash("b").as_str(), "open"));
    let p = payload(
        r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"},
            "metadata":{"bl-id":["bl-1a2b"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
}

#[test]
fn close_patches_state_once() {
    // close does not read the store (the ball file is gone post-retire); a dummy
    // store dir is fine.
    let store = tempfile::tempdir().unwrap();
    let terr = tempfile::tempdir().unwrap();
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/issues/7")
        .match_body(r#"{"state":"closed"}"#)
        .with_status(200)
        .with_body(r#"{"number":7,"html_url":"u","state":"closed"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let mut base = Base::default();
    base.set("bl-1a2b", snap(7, "T", "h", "open"));
    let p = payload(
        r#"{"op":"close","phase":"post","binding":{"invocation_path":"/p"},
            "metadata":{"bl-id":["bl-1a2b"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
    assert_eq!(base.get("bl-1a2b").unwrap().state, "closed");
}

#[test]
fn close_noops_when_unmapped_or_already_closed() {
    let store = tempfile::tempdir().unwrap();
    let terr = tempfile::tempdir().unwrap();
    let c = GithubClient::new("http://127.0.0.1:1", "t", UA); // would error if called
    let mut base = Base::default();
    // unmapped id
    let p = payload(
        r#"{"op":"close","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-x"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
    // already closed
    base.set("bl-y", snap(3, "T", "h", "closed"));
    let p2 = payload(
        r#"{"op":"close","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-y"]}}"#,
    );
    push(&p2, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
}

#[test]
fn missing_id_or_absent_ball_is_an_error() {
    let store = tempfile::tempdir().unwrap(); // empty: no tasks/<id>.md
    let terr = tempfile::tempdir().unwrap();
    let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
    let mut base = Base::default();
    let no_id = payload(r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"}}"#);
    assert!(push(&no_id, &c, "o", "n", &mut base, terr.path(), store.path()).is_err());
    let absent = payload(
        r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]}}"#,
    );
    assert!(push(&absent, &c, "o", "n", &mut base, terr.path(), store.path()).is_err());
}

#[test]
fn other_ops_noop() {
    let store = tempfile::tempdir().unwrap();
    let terr = tempfile::tempdir().unwrap();
    let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
    let mut base = Base::default();
    let p = payload(
        r#"{"op":"claim","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]}}"#,
    );
    push(&p, &c, "o", "n", &mut base, terr.path(), store.path()).unwrap();
}
