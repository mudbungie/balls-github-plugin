use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One entry of a task's `links` array as emitted by `bl list --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Link {
    pub link_type: String,
    pub target: String,
}

/// A balls task, as received on stdin. Only the fields the forge plugin
/// needs are decoded; everything else is ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub external: BTreeMap<String, Value>,
}

impl Task {
    pub fn pull_request(&self) -> Option<&Value> {
        self.external.get("github").and_then(|v| v.get("pull_request"))
    }

    pub fn pr_number(&self) -> Option<u64> {
        self.pull_request()
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_u64())
    }

    /// The id of the `gates` child auto-opened by `bl review` in deferred mode.
    pub fn gate_child_id(&self) -> Option<&str> {
        self.links
            .iter()
            .find(|l| l.link_type == "gates")
            .map(|l| l.target.as_str())
    }
}

/// `task.external.github` shape (stored verbatim by core after push).
#[derive(Debug, Serialize)]
pub struct PrRef {
    pub number: u64,
    pub url: String,
    pub head_sha: String,
    pub target_branch: String,
}

#[derive(Debug, Serialize)]
pub struct PushResponse {
    pub pull_request: PrRef,
}

/// Sync report. Only `updated` is ever populated by a forge plugin; an
/// empty report serialises to `{}`, which core accepts.
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
        assert!(t.pull_request().is_none());
        assert!(t.pr_number().is_none());
        assert!(t.gate_child_id().is_none());
    }

    #[test]
    fn pull_request_and_gate() {
        let t = task(
            r#"{"id":"bl-p","title":"t","status":"review",
                "links":[{"link_type":"relates_to","target":"bl-z"},
                         {"link_type":"gates","target":"bl-g"}],
                "external":{"github":{"pull_request":{"number":7}}}}"#,
        );
        assert!(t.pull_request().is_some());
        assert_eq!(t.pr_number(), Some(7));
        assert_eq!(t.gate_child_id(), Some("bl-g"));
    }

    #[test]
    fn pr_number_absent_when_not_numeric() {
        let t = task(
            r#"{"id":"bl-p","title":"t","status":"review",
                "external":{"github":{"pull_request":{"number":"x"}}}}"#,
        );
        assert_eq!(t.pr_number(), None);
    }

    #[test]
    fn serialize_push_response() {
        let r = PushResponse {
            pull_request: PrRef {
                number: 3,
                url: "https://gh/pr/3".into(),
                head_sha: "abc".into(),
                target_branch: "main".into(),
            },
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""pull_request":{"number":3"#));
    }

    #[test]
    fn serialize_sync_report() {
        assert_eq!(serde_json::to_string(&SyncReport::default()).unwrap(), "{}");
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
    }
}
