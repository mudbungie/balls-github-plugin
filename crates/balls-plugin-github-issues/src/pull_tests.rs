//! Tests for `pull.rs`. Lives in a sibling file (rather than the
//! inline `#[cfg(test)] mod tests` idiom) so `pull.rs` stays under
//! the 300-line cap. The split is mechanical; assertions are
//! unchanged.

use super::*;
use crate::pull_emit::created_from;
use serde_json::Value;

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
        pull_request: None,
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
            "external":{"github-issues":{"issue":{
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
            "external":{"github-issues":{"issue":{
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

// bl-2202 regression: when a GH issue title carries a `[bl-xxxx]`
// marker but the id is not in the task input (the original ball is
// closed/archived — balls `all_tasks` is open-only), the classifier
// must not AutoCreate. Otherwise the closed-mirror-re-ingest loop
// fires: a closed GH issue mirrored from balls is read back as new,
// a fresh ball gets created, push appends a second `[bl-yyyy]`, etc.
#[test]
fn orphaned_bl_tag_in_title_skips_instead_of_autocreating() {
    let i = issue(37, "Vendor SHA-1 [bl-cb4e]", "2026-01-01T00:00:00Z", &[]);
    assert_eq!(
        classify(&i, &[], &cfg(None)),
        Classification::Skip(SkipReason::OrphanedBlTag)
    );
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

// bl-b233 regression: `GET /repos/{o}/{n}/issues` returns PRs too,
// distinguished by a non-null `pull_request` sub-object on each entry.
// Without filtering, every release-plz PR was auto-created as a balls
// task and a later close rewrote the PR title on GH. The filter must
// drop the PR silently and pass the plain issue through.
#[test]
fn list_issues_drops_pull_request_entries() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_body(
            r#"[
                {"number":1,"title":"real issue","state":"open","html_url":"u",
                 "updated_at":"2026-01-01T00:00:00Z","labels":[]},
                {"number":2,"title":"a PR","state":"open","html_url":"u",
                 "updated_at":"2026-01-01T00:00:00Z","labels":[],
                 "pull_request":{"url":"https://api.github.com/repos/o/n/pulls/2"}}
            ]"#,
        )
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let issues = list_issues(&c, "o", "n").unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 1);
}

// bl-bb66 regression: GH defaults to 30 issues/page. An unpaginated
// listing made every off-page-1 mirrored task look externally-deleted
// (the delete-sweep flipped live tasks to `deferred`). list_issues
// must follow the `Link` rel="next" chain so the returned vec is the
// complete issue set across pages.
#[test]
fn list_issues_follows_pagination() {
    let mut s = mockito::Server::new();
    let next_url = format!("{}/repos/o/n/issues?state=all&per_page=100&page=2", s.url());
    // Page 2 mock first so it wins for the page=2 request; page 1
    // (no `page` param) falls through to the second, query-agnostic mock.
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .match_query(mockito::Matcher::UrlEncoded("page".into(), "2".into()))
        .with_status(200)
        .with_body(
            r#"[{"number":2,"title":"b","state":"open","html_url":"u",
                 "updated_at":"2026-01-01T00:00:00Z","labels":[]}]"#,
        )
        .create();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/issues".into()))
        .with_status(200)
        .with_header("link", &format!(r#"<{next_url}>; rel="next""#))
        .with_body(
            r#"[{"number":1,"title":"a","state":"open","html_url":"u",
                 "updated_at":"2026-01-01T00:00:00Z","labels":[]}]"#,
        )
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let nums: Vec<u64> = list_issues(&c, "o", "n")
        .unwrap()
        .iter()
        .map(|i| i.number)
        .collect();
    assert_eq!(nums, vec![1, 2]);
}

// next_page_url branch coverage: a `next` rel returns the bare URL;
// a header carrying only other rels (or a malformed semicolon-less
// part) yields None so the walk terminates.
#[test]
fn next_page_url_parsing() {
    assert_eq!(
        next_page_url(r#"<https://api/issues?page=2>; rel="next", <https://api/issues?page=9>; rel="last""#),
        Some("https://api/issues?page=2".to_string())
    );
    assert_eq!(
        next_page_url(r#"<https://api/issues?page=9>; rel="last""#),
        None
    );
    assert_eq!(next_page_url("garbage-no-semicolon"), None);
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

// bl-a2ea regression: a SyncCreate emitted by `created_from`, once
// wrapped by core under the participant name `github-issues`, must
// classify on the next poll as KnownUpdate (or Skip via loop
// avoidance) — never AutoCreate. The earlier double-wrap plus the
// underscore/hyphen key mismatch broke this round-trip and produced
// a duplicate-create on every sync.
#[test]
fn sync_create_round_trips_to_known_not_autocreate() {
    let i = issue(42, "External report", "2026-01-01T00:00:00Z", &[]);
    let create = created_from(&i);

    // Mimic balls-core's sync_report::apply_created: insert the
    // SyncCreate.external map verbatim under the participant name.
    let outer = serde_json::json!({
        "github-issues": Value::Object(create.external.clone()),
    });
    let task_json = serde_json::json!({
        "id": "bl-mirror",
        "title": create.title,
        "status": create.status,
        "external": outer,
    });
    let mirrored: Task = serde_json::from_value(task_json).unwrap();

    let cls = classify(&i, &[mirrored], &cfg(None));
    assert!(
        matches!(
            cls,
            Classification::KnownUpdate { .. }
                | Classification::Skip(SkipReason::LoopAvoidance)
        ),
        "expected KnownUpdate or Skip, got {cls:?}",
    );
}
