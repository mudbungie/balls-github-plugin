//! Plugin protocol types: the over-the-wire shape both plugins
//! deserialize (Task) and emit (SyncReport).
//!
//! Per-plugin `external.<name>.*` projections are NOT defined here —
//! each plugin's crate owns its projection. `Task::external` is a
//! free-form map; plugin-specific accessors live alongside the code
//! that uses them.

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
    /// Free-form labels. Issues plugin mirrors these to GH issue
    /// labels (B3 puts the field in place; B4c reads them on auto-
    /// create). Defaulted for forward-compat.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub external: BTreeMap<String, Value>,
}

/// Sync report. Plugins populate the variant(s) relevant to them: the
/// forge plugin uses only `updated`; the issues plugin populates all
/// three. Empty arrays are omitted from the JSON, so a no-op sync
/// serialises to `{}`, which core accepts as "nothing changed."
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<SyncUpdate>,
}

#[derive(Debug, Serialize)]
pub struct SyncUpdate {
    pub task_id: String,
    pub fields: BTreeMap<String, Value>,
    pub add_note: String,
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
            updated: vec![SyncUpdate {
                task_id: "bl-g".into(),
                fields,
                add_note: "merged".into(),
            }],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""task_id":"bl-g""#));
        assert!(s.contains(r#""status":"closed""#));
        assert!(s.contains(r#""add_note":"merged""#));
    }
}
