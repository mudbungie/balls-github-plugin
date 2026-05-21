//! Plugin protocol types: the over-the-wire shape both plugins
//! deserialize (Task) and emit (SyncReport).
//!
//! Per-plugin `external.<name>.*` projections are NOT defined here —
//! each plugin's crate owns its projection. `Task::external` is a
//! free-form map; plugin-specific accessors live alongside the code
//! that uses them.
//!
//! The SyncReport / SyncCreate / SyncUpdate / SyncDelete shapes
//! mirror balls-core's `plugin::types::SyncReport` exactly: the
//! plugin serializes to a shape core deserializes, so the field
//! names and defaults must agree byte-for-byte.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One entry of a task's `links` array as emitted by `bl list --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Link {
    pub link_type: String,
    pub target: String,
}

/// A balls task as received on stdin. Only the fields plugins commonly
/// need are decoded; everything else is ignored (serde drops unknown
/// keys, which is also the SPEC §13 forward-compat rule for plugins).
#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: String,
    /// Long-form body. Issues plugin mirrors this to the GH issue body
    /// (B3); forge plugin doesn't use it. Defaulted so older balls
    /// task files without this field still decode.
    #[serde(default)]
    pub description: String,
    /// Free-form labels. Issues plugin mirrors these to/from GH issue
    /// labels (B3 wrote the type slot; B4c populates on auto-create).
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub external: BTreeMap<String, Value>,
}

/// Full sync report returned by a plugin on stdout after `sync`.
/// Empty arrays are omitted, so a no-op sync serialises to `{}`,
/// which core accepts as "nothing changed".
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<SyncCreate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<SyncUpdate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<SyncDelete>,
}

/// A new task to create locally, reported by plugin sync. Field
/// shape and defaults mirror balls-core's SyncCreate exactly.
#[derive(Debug, Serialize)]
pub struct SyncCreate {
    pub title: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub priority: u8,
    pub status: String,
    pub description: String,
    pub tags: Vec<String>,
    pub external: serde_json::Map<String, Value>,
}

/// Fields to update on an existing local task.
#[derive(Debug, Serialize)]
pub struct SyncUpdate {
    pub task_id: String,
    pub fields: BTreeMap<String, Value>,
    /// Replacement value for `task.external.<plugin_name>`. Non-empty
    /// means "rewrite my projection blob with this exact map";
    /// `is_empty` means "leave the projection untouched" (so a sync
    /// that only flips a top-level field doesn't accidentally clobber
    /// the projection a previous push wrote). Mirrors core's
    /// `SyncUpdate.external` shape exactly.
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub external: serde_json::Map<String, Value>,
    /// `add_note` is kept as a plain String here (rather than the
    /// `Option<String>` core uses) because every emit-site has a
    /// concrete note to write. Core's deserializer accepts a bare
    /// string as Some(_).
    pub add_note: String,
}

/// A local task to mark as deferred (per core's convention).
#[derive(Debug, Serialize)]
pub struct SyncDelete {
    pub task_id: String,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn minimal_task_defaults() {
        let t = task(r#"{"id":"bl-x","title":"t","status":"open"}"#);
        assert!(t.links.is_empty());
        assert!(t.external.is_empty());
        assert!(t.target_branch.is_none());
    }

    #[test]
    fn links_and_external_round_trip() {
        let t = task(
            r#"{"id":"bl-p","title":"t","status":"review",
                "links":[{"link_type":"gates","target":"bl-g"}],
                "external":{"github":{"pull_request":{"number":7}}}}"#,
        );
        assert_eq!(t.links.len(), 1);
        assert_eq!(t.links[0].link_type, "gates");
        assert_eq!(t.links[0].target, "bl-g");
        assert!(t.external.contains_key("github"));
    }

    #[test]
    fn serialize_sync_report_empty() {
        assert_eq!(serde_json::to_string(&SyncReport::default()).unwrap(), "{}");
    }

    #[test]
    fn serialize_sync_report_with_update() {
        let mut fields = BTreeMap::new();
        fields.insert("status".into(), Value::String("closed".into()));
        let r = SyncReport {
            created: Vec::new(),
            updated: vec![SyncUpdate {
                task_id: "bl-g".into(),
                fields,
                external: serde_json::Map::new(),
                add_note: "merged".into(),
            }],
            deleted: Vec::new(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""task_id":"bl-g""#));
        assert!(s.contains(r#""status":"closed""#));
        assert!(s.contains(r#""add_note":"merged""#));
        // empty created/deleted vecs are omitted
        assert!(!s.contains(r#""created":["#));
        assert!(!s.contains(r#""deleted":["#));
    }

    #[test]
    fn serialize_sync_report_with_create() {
        let r = SyncReport {
            created: vec![SyncCreate {
                title: "From GH".into(),
                task_type: "task".into(),
                priority: 3,
                status: "open".into(),
                description: "body".into(),
                tags: vec!["bug".into()],
                external: serde_json::Map::new(),
            }],
            updated: Vec::new(),
            deleted: Vec::new(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""title":"From GH""#));
        assert!(s.contains(r#""type":"task""#));
        assert!(s.contains(r#""priority":3"#));
    }

    #[test]
    fn serialize_sync_update_external_round_trip() {
        let mut ext = serde_json::Map::new();
        ext.insert("issue".into(), serde_json::json!({"number": 9}));
        let r = SyncReport {
            created: Vec::new(),
            updated: vec![SyncUpdate {
                task_id: "bl-e".into(),
                fields: BTreeMap::new(),
                external: ext,
                add_note: "n".into(),
            }],
            deleted: Vec::new(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""external":{"issue":{"number":9}}"#));
    }

    #[test]
    fn serialize_sync_report_with_delete() {
        let r = SyncReport {
            created: Vec::new(),
            updated: Vec::new(),
            deleted: vec![SyncDelete {
                task_id: "bl-g".into(),
                reason: "gone".into(),
            }],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""task_id":"bl-g""#));
        assert!(s.contains(r#""reason":"gone""#));
    }
}
