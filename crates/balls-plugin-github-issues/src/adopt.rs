//! One-time legacy adoption (§16/bl-2a81, reworked under bl-0ef9) — stamp the
//! `[bl-xxxx]` issue-title MARKER onto each pre-greenfield GitHub issue so the
//! first greenfield `sync` re-adopts it with ZERO duplication.
//!
//! Legacy balls kept the task↔issue join inline on the task as
//! `external.github-issues.issue.number`. The §16 base migrator DROPS that blob
//! (it is plugin territory, not a core field), so a naive cutover would leave
//! the greenfield store with no record of which issue each task owns — the next
//! `sync` would see marker-less issues and AUTO-CREATE a duplicate for each.
//!
//! The join SSOT is the `[bl-xxxx]` marker on the issue title (`crate::marker`,
//! bl-613d), not the territory base. So adoption amends the SSOT directly: for
//! each legacy task carrying an issue number it appends `[bl-id]` to that
//! issue's title (one PATCH). The marker lives on GitHub, visible to EVERY
//! clone, so federation is free — a clone that never ran `adopt` still joins via
//! the marker on its first `sync` (pull priority 1). The reconciliation base
//! (`crate::base`) stays a rebuildable sync-cache; adoption never writes it.
//!
//! This runs ONLINE (it reads the live issue title and PATCHes it, reusing the
//! shared GitHub client + token like `auth-check`) and is invoked once by the
//! §16 cutover runbook (bl-0802) AFTER the base migrator and `bl prime`, from
//! the project directory (the cwd that keys the plugin territory).

use std::path::Path;

use balls_github_shared::error::Result;
use balls_github_shared::http::GithubClient;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{issues_api, marker};

/// The slice of a legacy (pre-greenfield) task JSON this adoption reads: the id
/// (a greenfield ball id is the same `bl-xxxx`), the status (to skip closed),
/// and the free-form `external` projection that held the issue number. serde
/// drops every other legacy field.
#[derive(Deserialize)]
struct LegacyTask {
    id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    external: Map<String, Value>,
}

impl LegacyTask {
    /// The mirrored GitHub issue number from `external.github-issues.issue.number`,
    /// if this task was ever pushed to an issue.
    fn issue_number(&self) -> Option<u64> {
        self.external.get("github-issues")?.get("issue")?.get("number")?.as_u64()
    }
}

/// What an adoption run did, for the operator-facing summary.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Issues now carrying the `[bl-id]` marker (PATCHed this run, or already
    /// correct from a prior run — both are joined).
    pub stamped: usize,
    /// Legacy tasks skipped: closed (absence = closed, §9) or never mirrored.
    pub skipped: usize,
}

/// Whether one legacy task got its marker stamped or was skipped.
enum Outcome {
    Stamped,
    Skipped,
}

/// Stamp the `[bl-xxxx]` marker onto every mirrored legacy issue in `legacy_dir`.
/// Reads each issue's live title and PATCHes the marker on (idempotent: an
/// already-correct title makes no API call). A malformed `*.json` or a GitHub
/// error aborts; since nothing local is written, a re-run is safe and re-stamps
/// no-op the issues already done.
pub fn adopt(legacy_dir: &Path, client: &GithubClient, owner: &str, name: &str) -> Result<Summary> {
    let mut summary = Summary::default();
    for entry in std::fs::read_dir(legacy_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue; // not a task file — ignore quietly
        }
        let task: LegacyTask = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        match stamp_one(client, owner, name, &task)? {
            Outcome::Stamped => summary.stamped += 1,
            Outcome::Skipped => summary.skipped += 1,
        }
    }
    Ok(summary)
}

/// Stamp one task's marker. Skips a closed task (greenfield keeps no file for it
/// — absence is closed, §9) and a task that was never mirrored to an issue.
/// Otherwise reads the issue's current title and PATCHes the `[bl-id]` marker
/// on, no-opping the PATCH when the title is already correct.
fn stamp_one(client: &GithubClient, owner: &str, name: &str, task: &LegacyTask) -> Result<Outcome> {
    if task.status == "closed" {
        return Ok(Outcome::Skipped);
    }
    let Some(number) = task.issue_number() else {
        return Ok(Outcome::Skipped);
    };
    let issue = issues_api::get_issue(client, owner, name, number)?;
    let marked = marker::append(&issue.title, &task.id);
    if marked != issue.title {
        issues_api::patch(client, owner, name, number, &json!({ "title": marked }))?;
    }
    Ok(Outcome::Stamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-issues-test";

    fn write(dir: &Path, name: &str, json: &str) {
        std::fs::write(dir.join(name), json).unwrap();
    }

    /// A legacy task JSON with a mirrored issue number.
    fn legacy(id: &str, status: &str, number: u64) -> String {
        format!(
            r#"{{"id":"{id}","status":"{status}",
                "external":{{"github-issues":{{"issue":{{"number":{number},"state":"open"}}}}}}}}"#
        )
    }

    #[test]
    fn stamps_the_marker_on_a_marker_less_issue() {
        let legacy_dir = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "bl-1a2b.json", &legacy("bl-1a2b", "open", 7));
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/issues/7")
            .with_status(200)
            .with_body(r#"{"number":7,"title":"Old issue","state":"open"}"#)
            .create();
        s.mock("PATCH", "/repos/o/n/issues/7")
            .match_body(r#"{"title":"Old issue [bl-1a2b]"}"#)
            .with_status(200)
            .with_body(r#"{"number":7,"title":"Old issue [bl-1a2b]","state":"open"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);

        let summary = adopt(legacy_dir.path(), &c, "o", "n").unwrap();
        assert_eq!(summary, Summary { stamped: 1, skipped: 0 });
    }

    #[test]
    fn an_already_marked_issue_makes_no_patch() {
        let legacy_dir = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "bl-1a2b.json", &legacy("bl-1a2b", "open", 7));
        let mut s = mockito::Server::new();
        // GET shows the marker is already present; no PATCH is mocked, so any
        // PATCH call would 501 and fail the run — proving idempotency.
        s.mock("GET", "/repos/o/n/issues/7")
            .with_status(200)
            .with_body(r#"{"number":7,"title":"Old issue [bl-1a2b]","state":"open"}"#)
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);

        let summary = adopt(legacy_dir.path(), &c, "o", "n").unwrap();
        assert_eq!(summary, Summary { stamped: 1, skipped: 0 });
    }

    #[test]
    fn skips_closed_and_never_mirrored() {
        let legacy_dir = tempfile::tempdir().unwrap();
        // closed → absence is closed, no greenfield ball to join
        write(legacy_dir.path(), "bl-dead.json", &legacy("bl-dead", "closed", 1));
        // never mirrored → no issue number to stamp
        write(legacy_dir.path(), "bl-bare.json", r#"{"id":"bl-bare","status":"open"}"#);
        // no GitHub call is expected for either; an unreachable server would error
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);

        let summary = adopt(legacy_dir.path(), &c, "o", "n").unwrap();
        assert_eq!(summary, Summary { stamped: 0, skipped: 2 });
    }

    #[test]
    fn ignores_non_json_files() {
        let legacy_dir = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "README.md", "not a task");
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
        let summary = adopt(legacy_dir.path(), &c, "o", "n").unwrap();
        assert_eq!(summary, Summary { stamped: 0, skipped: 0 });
    }

    #[test]
    fn issue_number_reads_every_missing_rung_as_none() {
        let no_ext: LegacyTask = serde_json::from_str(r#"{"id":"a","status":"open"}"#).unwrap();
        assert_eq!(no_ext.issue_number(), None);
        let no_issue: LegacyTask =
            serde_json::from_str(r#"{"id":"a","external":{"github-issues":{}}}"#).unwrap();
        assert_eq!(no_issue.issue_number(), None);
        let no_number: LegacyTask =
            serde_json::from_str(r#"{"id":"a","external":{"github-issues":{"issue":{}}}}"#).unwrap();
        assert_eq!(no_number.issue_number(), None);
        let not_u64: LegacyTask = serde_json::from_str(
            r#"{"id":"a","external":{"github-issues":{"issue":{"number":"x"}}}}"#,
        )
        .unwrap();
        assert_eq!(not_u64.issue_number(), None);
    }

    #[test]
    fn a_github_error_aborts() {
        let legacy_dir = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "bl-1.json", &legacy("bl-1", "open", 5));
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/issues/5").with_status(404).with_body("gone").create();
        let c = GithubClient::new(&s.url(), "t", UA);
        assert!(adopt(legacy_dir.path(), &c, "o", "n").is_err());
    }

    #[test]
    fn a_malformed_json_aborts() {
        let legacy_dir = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "bl-bad.json", "not json");
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
        assert!(adopt(legacy_dir.path(), &c, "o", "n").is_err());
    }

    #[test]
    fn a_missing_legacy_dir_is_an_error() {
        let c = GithubClient::new("http://127.0.0.1:1", "t", UA);
        assert!(adopt(Path::new("/no/such/dir"), &c, "o", "n").is_err());
    }
}
