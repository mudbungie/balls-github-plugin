//! Tests for `pull.rs`. Lives in a sibling file (rather than the
//! inline `#[cfg(test)] mod tests` idiom) so `pull.rs` stays under
//! the 300-line cap. The split is mechanical; assertions are
//! unchanged.

use super::*;

const UA: &str = "balls-plugin-github-issues-test";

fn cfg(label: Option<&str>) -> PluginConfig {
    let extra = label
        .map(|l| format!(r#","target_label":{:?}"#, l))
        .unwrap_or_default();
    serde_json::from_str(&format!(r#"{{"repo":"o/n"{}}}"#, extra)).unwrap()
}

fn issue(num: u64, title: &str, updated_at: &str, labels: &[&str]) -> GhIssue {
    GhIssue {
        number: num,
        title: title.into(),
        body: None,
        state: "open".into(),
        html_url: "u".into(),
        updated_at: updated_at.into(),
        labels: labels
            .iter()
            .map(|n| GhLabel { name: (*n).into() })
            .collect(),
    }
}

fn task(json: &str) -> Task {
    serde_json::from_str(json).unwrap()
}

#[test]
fn matches_by_stored_number() {
    let i = issue(7, "anything", "2026-01-02T00:00:00Z", &[]);
    let t = task(
        r#"{"id":"bl-1","title":"t","status":"open",
            "external":{"github_issues":{"issue":{
                "number":7,"url":"u","state":"open",
                "source":"balls","synced_at":"2026-01-01T00:00:00+00:00",
                "last_synced_status":"open"}}}}"#,
    );
    assert_eq!(
        classify(&i, &[t], &cfg(None)),
        Classification::KnownUpdate {
            task_id: "bl-1".into()
        }
    );
}

#[test]
fn skips_when_synced_covers_update() {
    let i = issue(7, "any", "2026-01-01T00:00:00Z", &[]);
    let t = task(
        r#"{"id":"bl-1","title":"t","status":"open",
            "external":{"github_issues":{"issue":{
                "number":7,"url":"u","state":"open",
                "source":"balls","synced_at":"2026-01-02T00:00:00+00:00",
                "last_synced_status":"open"}}}}"#,
    );
    assert_eq!(
        classify(&i, &[t], &cfg(None)),
        Classification::Skip(SkipReason::LoopAvoidance)
    );
}

#[test]
fn matches_by_title_tag_when_no_number() {
    let i = issue(99, "Title [bl-1a2b]", "2026-01-01T00:00:00Z", &[]);
    let t = task(r#"{"id":"bl-1a2b","title":"t","status":"open"}"#);
    assert_eq!(
        classify(&i, &[t], &cfg(None)),
        Classification::KnownUpdate {
            task_id: "bl-1a2b".into()
        }
    );
}

#[test]
fn unmatched_issue_becomes_autocreate() {
    let i = issue(99, "External report", "2026-01-01T00:00:00Z", &[]);
    assert_eq!(classify(&i, &[], &cfg(None)), Classification::AutoCreate);
}

#[test]
fn label_filter_skips_non_matching_issues() {
    let i = issue(99, "Without label", "2026-01-01T00:00:00Z", &[]);
    assert_eq!(
        classify(&i, &[], &cfg(Some("balls:track"))),
        Classification::Skip(SkipReason::LabelFilter)
    );

    let i2 = issue(
        99,
        "With label",
        "2026-01-01T00:00:00Z",
        &["other", "balls:track"],
    );
    assert_eq!(
        classify(&i2, &[], &cfg(Some("balls:track"))),
        Classification::AutoCreate
    );
}

#[test]
fn malformed_title_tag_falls_through_to_autocreate() {
    assert!(extract_bl_id("nope [bl-xxxx]").is_none());
    assert!(extract_bl_id("nope [bl-12]").is_none());
    assert!(extract_bl_id("bl-1a2b").is_none());
    assert_eq!(extract_bl_id("T [bl-1a2b]").as_deref(), Some("bl-1a2b"));
    assert_eq!(
        extract_bl_id("T [bl-1a2b3c4d]").as_deref(),
        Some("bl-1a2b3c4d")
    );
}

#[test]
fn unparseable_timestamps_do_not_skip() {
    assert!(!synced_covers_update("not-a-date", "2026-01-01T00:00:00Z"));
    assert!(!synced_covers_update("2026-01-01T00:00:00Z", "not-a-date"));
}

#[test]
fn list_issues_round_trip() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":1,"title":"a","state":"open","html_url":"u",
                 "updated_at":"2026-01-01T00:00:00Z","labels":[]}]"#,
        )
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let issues = list_issues(&c, "o", "n").unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 1);
}

#[test]
fn list_issues_propagates_api_error() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(503)
        .with_body("down")
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    assert!(list_issues(&c, "o", "n").is_err());
}

// Internal-helper coverage: extract_bl_id has a couple of branches
// that the malformed_title_tag test exercises but the `find(']')?`
// short-circuit needs its own case to cover the "no closing
// bracket after [bl-" path.
#[test]
fn extract_bl_id_no_closing_bracket() {
    assert!(extract_bl_id("Title [bl-1a2b open issue").is_none());
}
