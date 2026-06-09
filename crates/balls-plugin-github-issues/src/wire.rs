//! §7 inbound payload — the slice of the plugin wire this plugin reads.
//!
//! balls serializes a §7 `Payload` to the plugin's stdin and deserializes
//! NOTHING back (the no-return-channel rule). So this is an input-only mirror of
//! core's `wire::Payload`: serde silently drops every wire field we do not name,
//! which keeps the type stable as the wire grows AND trims it to exactly what
//! this plugin consumes. The op and phase arrive on argv (`<bin> <op> <phase>`,
//! §6); the payload carries the binding (the `store` checkout push and pull both
//! read the ball from) and, on `post`, the sealed `bl-id` in the §5 `metadata`
//! trailers. The op's content is NOT taken from the wire: both directions read
//! the sealed ball's title and body from the store (`crate::store`), the single
//! source of truth (bl-68db).

use serde::Deserialize;
use std::collections::BTreeMap;

/// §7 binding — only the three paths this plugin needs. `landing` is the
/// `balls/config` checkout that holds the plugin config (§1/§4); `store` is the
/// `tasks/` checkout BOTH directions read the ball from (ff-merged to the seal
/// before `post`, §8); `invocation_path` is the project root that keys the
/// plugin's territory and is the cwd the shelled `bl` runs in.
#[derive(Debug, Clone, Deserialize)]
pub struct Binding {
    #[serde(default)]
    pub landing: String,
    #[serde(default)]
    pub store: String,
    pub invocation_path: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Payload {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn reads_a_post_payload_with_id_and_binding() {
        // The wire still carries `command`/`current_state`/`previous_state`; serde
        // drops them silently — this plugin reads content from the store, not here.
        let p = parse(
            r#"{"op":"update","phase":"post","actor":"me",
                "binding":{"remote":"x","tasks_branch":"balls/tasks","store":"/s","landing":"/l","invocation_path":"/proj"},
                "command":{"op":"update","field_changes":[{"field":"title"}],"body_change":"hello"},
                "current_state":{"title":"New [bl-1a2b]","tags":["bug"]},
                "previous_state":{"title":"Old"},
                "metadata":{"bl-id":["bl-1a2b"],"bl-op":["update"]}}"#,
        );
        assert_eq!(p.op, "update");
        assert_eq!(p.actor, "me");
        assert_eq!(p.binding.landing, "/l");
        assert_eq!(p.binding.store, "/s");
        assert_eq!(p.binding.invocation_path, "/proj");
        assert_eq!(p.id(), Some("bl-1a2b"));
    }

    #[test]
    fn a_pre_payload_has_no_id_or_metadata() {
        let p = parse(
            r#"{"op":"create","phase":"pre","binding":{"invocation_path":"/p"},"command":{"op":"create"}}"#,
        );
        assert!(p.id().is_none());
        assert_eq!(p.binding.invocation_path, "/p");
        assert_eq!(p.binding.landing, ""); // defaulted
    }

    #[test]
    fn a_diffless_sync_payload_decodes() {
        let p = parse(
            r#"{"op":"sync","phase":"post","binding":{"store":"/s","landing":"/l","invocation_path":"/p"}}"#,
        );
        assert_eq!(p.op, "sync");
        assert_eq!(p.binding.store, "/s");
        assert!(p.id().is_none());
    }
}
