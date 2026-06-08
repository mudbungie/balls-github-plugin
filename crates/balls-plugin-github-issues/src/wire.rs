//! §7 inbound payload — the slice of the plugin wire this plugin reads.
//!
//! balls serializes a §7 `Payload` to the plugin's stdin and deserializes
//! NOTHING back (the no-return-channel rule). So this is an input-only mirror of
//! core's `wire::Payload`: serde silently drops every wire field we do not name,
//! which keeps the type stable as the wire grows AND trims it to exactly what
//! this plugin consumes. The op and phase arrive on argv (`<bin> <op> <phase>`,
//! §6); the payload carries the binding, the staged body, the after-state title,
//! and (post) the sealed `bl-id` in the §5 `metadata` trailers.

use serde::Deserialize;
use std::collections::BTreeMap;

/// §7 binding — only the three paths this plugin needs. `landing` is the
/// `balls/config` checkout that holds the plugin config (§1/§4); `store` is the
/// `tasks/` checkout the pull side reads; `invocation_path` is the project root
/// that keys the plugin's territory and is the cwd the shelled `bl` runs in.
#[derive(Debug, Clone, Deserialize)]
pub struct Binding {
    #[serde(default)]
    pub landing: String,
    #[serde(default)]
    pub store: String,
    pub invocation_path: String,
}

/// §7 command — only `body_change`, the new markdown body when an op rewrites
/// it. This is the ONLY way the body reaches the plugin: core skip-serializes
/// `Task::body` (§3), so the task states never carry it.
#[derive(Debug, Clone, Deserialize)]
pub struct Command {
    #[serde(default)]
    pub body_change: Option<String>,
}

/// A greenfield task as it rides the §7 wire (`current_state`). Only `title` is
/// needed — no `status`/`id`/`body` (§3: status derived, id is the filename and
/// arrives as the `bl-id` metadata trailer, body skip-serialized).
#[derive(Debug, Clone, Deserialize)]
pub struct WireTask {
    pub title: String,
}

/// One §7 payload as received on stdin, trimmed to the consumed fields. Absent
/// optionals default, so the same type decodes `pre`/`post`/`sync` shapes.
#[derive(Debug, Clone, Deserialize)]
pub struct Payload {
    pub op: String,
    /// The invoking identity (`--as`); the pull side stamps shelled verbs with it.
    #[serde(default)]
    pub actor: String,
    pub binding: Binding,
    #[serde(default)]
    pub command: Option<Command>,
    /// `post`: the sealed after-state (its `title`). Absent on `pre`/diffless.
    #[serde(default)]
    pub current_state: Option<WireTask>,
    /// §5 trailers parsed from the seal commit, incl. the sealed `bl-id`.
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, Vec<String>>>,
}

impl Payload {
    /// The sealed ball id, from the post `metadata` `bl-id` trailer.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.metadata.as_ref()?.get("bl-id")?.first().map(String::as_str)
    }

    /// The new markdown body this op stages, if any (`command.body_change`).
    #[must_use]
    pub fn body_change(&self) -> Option<&str> {
        self.command.as_ref()?.body_change.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Payload {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn reads_a_post_payload_with_id_and_body() {
        let p = parse(
            r#"{"op":"update","phase":"post","actor":"me",
                "binding":{"remote":"x","tasks_branch":"balls/tasks","store":"/s","landing":"/l","invocation_path":"/proj"},
                "command":{"op":"update","field_changes":[{"field":"title"}],"body_change":"hello"},
                "current_state":{"title":"New [bl-1a2b]","tags":["bug"]},
                "previous_state":{"title":"Old"},
                "metadata":{"bl-id":["bl-1a2b"],"bl-op":["update"]}}"#,
        );
        assert_eq!(p.op, "update");
        assert_eq!(p.binding.landing, "/l");
        assert_eq!(p.binding.store, "/s");
        assert_eq!(p.binding.invocation_path, "/proj");
        assert_eq!(p.id(), Some("bl-1a2b"));
        assert_eq!(p.body_change(), Some("hello"));
        assert_eq!(p.current_state.unwrap().title, "New [bl-1a2b]");
    }

    #[test]
    fn a_pre_payload_has_no_id_or_metadata() {
        let p = parse(
            r#"{"op":"create","phase":"pre","binding":{"invocation_path":"/p"},"command":{"op":"create"}}"#,
        );
        assert!(p.id().is_none());
        assert!(p.body_change().is_none());
        assert!(p.current_state.is_none());
        assert_eq!(p.binding.invocation_path, "/p");
        assert_eq!(p.binding.landing, ""); // defaulted
    }

    #[test]
    fn a_diffless_sync_payload_decodes() {
        let p = parse(
            r#"{"op":"sync","phase":"post","binding":{"store":"/s","landing":"/l","invocation_path":"/p"}}"#,
        );
        assert_eq!(p.op, "sync");
        assert!(p.command.is_none());
        assert!(p.id().is_none());
    }
}
