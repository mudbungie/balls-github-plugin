//! Push (balls → GitHub), the `*.post` direction (bl-613d).
//!
//! Wired on `create`/`update`/`close`/`drop` `post`. Reads the §7 post payload —
//! the sealed `bl-id` (from `metadata`), the after-state title, and the staged
//! body (`command.body_change`) — and reconciles the mapped GitHub issue. The
//! reconciliation base (`crate::base`) is both the number cache (so a PATCH
//! needs no repo listing) and the idempotency oracle: an op that did not move
//! the title/body/state makes no API call, which is what closes the
//! pull→`bl update`→push loop (the base was advanced during the pull).
//!
//! When the import guard is set the handler suppresses itself entirely — a
//! pull-driven create/close must not echo back out to GitHub
//! (`crate::shellback`).

use std::path::Path;

use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use serde_json::{json, Value};

use crate::base::{Base, Snapshot};
use crate::content::body_hash;
use crate::issues_api;
use crate::marker;
use crate::wire::Payload;

/// Reconcile the mapped GitHub issue for one sealed task op. `base` is mutated
/// and persisted to `territory` on any change.
pub fn push(
    payload: &Payload,
    client: &GithubClient,
    owner: &str,
    name: &str,
    base: &mut Base,
    territory: &Path,
) -> Result<()> {
    let id = payload
        .id()
        .ok_or_else(|| PluginError::Other("post payload carries no bl-id".into()))?;
    match payload.op.as_str() {
        "close" | "drop" => close_issue(client, owner, name, id, base, territory),
        "create" | "update" => upsert(payload, client, owner, name, id, base, territory),
        _ => Ok(()),
    }
}

/// Close the mapped issue (the retire ops). No-op if the task was never
/// mirrored, or its issue is already closed (idempotent).
fn close_issue(
    client: &GithubClient,
    owner: &str,
    name: &str,
    id: &str,
    base: &mut Base,
    territory: &Path,
) -> Result<()> {
    let Some(snap) = base.get(id).cloned() else {
        return Ok(());
    };
    if snap.state == "closed" {
        return Ok(());
    }
    issues_api::patch(client, owner, name, snap.number, &json!({ "state": "closed" }))?;
    base.set(id, Snapshot { state: "closed".into(), ..snap });
    base.save(territory)?;
    Ok(())
}

/// Create or update the mapped issue from the after-state. A new task POSTs a
/// fresh issue (outward mirror-on-create); an existing one PATCHes only when the
/// title, body, or state actually moved.
fn upsert(
    payload: &Payload,
    client: &GithubClient,
    owner: &str,
    name: &str,
    id: &str,
    base: &mut Base,
    territory: &Path,
) -> Result<()> {
    let task = payload
        .current_state
        .as_ref()
        .ok_or_else(|| PluginError::Other("create/update post has no current_state".into()))?;
    let bare = marker::strip(&task.title).0.to_string();
    let body_change = payload.body_change();
    let marked = marker::append(&bare, id);

    match base.get(id).cloned() {
        Some(snap) => {
            let title_moved = bare != snap.title;
            let body_moved = body_change.is_some_and(|b| body_hash(b) != snap.body_hash);
            let reopen = snap.state != "open";
            if !(title_moved || body_moved || reopen) {
                return Ok(()); // nothing moved — loop-avoidance no-op
            }
            let mut fields = serde_json::Map::new();
            fields.insert("title".into(), Value::String(marked));
            fields.insert("state".into(), Value::String("open".into()));
            if let Some(b) = body_change {
                fields.insert("body".into(), Value::String(b.to_string()));
            }
            issues_api::patch(client, owner, name, snap.number, &Value::Object(fields))?;
            let body_hash_now = body_change.map_or(snap.body_hash, body_hash);
            base.set(id, Snapshot { number: snap.number, title: bare, body_hash: body_hash_now, state: "open".into() });
        }
        None => {
            let body = body_change.unwrap_or("");
            let issue = issues_api::create_issue(client, owner, name, &marked, body)?;
            base.set(id, Snapshot { number: issue.number, title: bare, body_hash: body_hash(body), state: issue.state });
        }
    }
    base.save(territory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-issues-test";

    fn payload(json: &str) -> Payload {
        serde_json::from_str(json).unwrap()
    }

    fn snap(number: u64, title: &str, body_hash: &str, state: &str) -> Snapshot {
        Snapshot { number, title: title.into(), body_hash: body_hash.into(), state: state.into() }
    }

    #[test]
    fn create_posts_a_new_issue_and_records_the_base() {
        let dir = tempfile::tempdir().unwrap();
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
                "command":{"op":"create","body_change":"the body"},
                "current_state":{"title":"Hello"},
                "metadata":{"bl-id":["bl-1a2b"]}}"#,
        );
        push(&p, &c, "o", "n", &mut base, dir.path()).unwrap();
        let snap = base.get("bl-1a2b").unwrap();
        assert_eq!(snap.number, 7);
        assert_eq!(snap.title, "Hello");
        assert_eq!(snap.state, "open");
        // persisted
        assert_eq!(Base::load(dir.path()).unwrap().get("bl-1a2b").unwrap().number, 7);
    }

    #[test]
    fn update_patches_when_title_moves() {
        let dir = tempfile::tempdir().unwrap();
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
                "command":{"op":"update","body_change":"fresh body"},
                "current_state":{"title":"New"},
                "metadata":{"bl-id":["bl-1a2b"]}}"#,
        );
        push(&p, &c, "o", "n", &mut base, dir.path()).unwrap();
        assert_eq!(base.get("bl-1a2b").unwrap().title, "New");
    }

    #[test]
    fn update_noops_when_nothing_moved() {
        let dir = tempfile::tempdir().unwrap();
        // No mock server hit: if push made any call it would error (no server).
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
        let mut base = Base::default();
        base.set("bl-1a2b", snap(7, "Same", body_hash("b").as_str(), "open"));
        let p = payload(
            r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"},
                "command":{"op":"update","body_change":"b"},
                "current_state":{"title":"Same"},
                "metadata":{"bl-id":["bl-1a2b"]}}"#,
        );
        push(&p, &c, "o", "n", &mut base, dir.path()).unwrap();
    }

    #[test]
    fn close_patches_state_once() {
        let dir = tempfile::tempdir().unwrap();
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
        push(&p, &c, "o", "n", &mut base, dir.path()).unwrap();
        assert_eq!(base.get("bl-1a2b").unwrap().state, "closed");
    }

    #[test]
    fn close_noops_when_unmapped_or_already_closed() {
        let dir = tempfile::tempdir().unwrap();
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA); // would error if called
        let mut base = Base::default();
        // unmapped id
        let p = payload(
            r#"{"op":"close","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-x"]}}"#,
        );
        push(&p, &c, "o", "n", &mut base, dir.path()).unwrap();
        // already closed
        base.set("bl-y", snap(3, "T", "h", "closed"));
        let p2 = payload(
            r#"{"op":"close","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-y"]}}"#,
        );
        push(&p2, &c, "o", "n", &mut base, dir.path()).unwrap();
    }

    #[test]
    fn missing_id_or_state_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
        let mut base = Base::default();
        let no_id = payload(r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"}}"#);
        assert!(push(&no_id, &c, "o", "n", &mut base, dir.path()).is_err());
        let no_state = payload(
            r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]}}"#,
        );
        assert!(push(&no_state, &c, "o", "n", &mut base, dir.path()).is_err());
    }

    #[test]
    fn other_ops_noop() {
        let dir = tempfile::tempdir().unwrap();
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
        let mut base = Base::default();
        let p = payload(
            r#"{"op":"claim","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]}}"#,
        );
        push(&p, &c, "o", "n", &mut base, dir.path()).unwrap();
    }
}
