//! One-time legacy adoption (§16/bl-2a81) — seed the reconciliation base
//! (`crate::base`) from a pre-greenfield task store so the first greenfield
//! `sync` re-adopts existing GitHub issues with ZERO duplication.
//!
//! Legacy balls kept the task↔issue join inline on the task as
//! `external.github-issues.issue.number`. The §16 base migrator DROPS that blob
//! (it is plugin territory, not a core field), so a naive cutover would leave
//! the greenfield store with no record of which issue each task owns — the next
//! `sync` would see marker-less issues with no base link and AUTO-CREATE a
//! duplicate for every one. The marker is the join SSOT, but legacy issues carry
//! no `[bl-xxxx]` marker; the base's reverse lookup (`Base::id_for_number`) is
//! the documented fallback for exactly that case (a GH title with no marker but a
//! base that still remembers the link). This adoption seeds that fallback.
//!
//! migrate-clean-or-delink (§16): seed ONLY the one fact the legacy store proves
//! — the issue number. The last-agreed title/body/state are unknown (legacy kept
//! no agreed-content hash), so they are seeded as force-refresh sentinels
//! (`""`/`""`/`"open"`): the first `sync` reads GitHub directly and overwrites
//! the snapshot with the real values; an empty title/body never suppresses an
//! outward content re-assert, and `open` never suppresses an outward close.
//!
//! This runs OFFLINE (it never touches GitHub) and is invoked once by the §16
//! cutover runbook (bl-0802) AFTER the base migrator and `bl prime`, from the
//! project directory (the cwd that keys the plugin territory, like `auth-setup`).

use std::path::Path;

use balls_github_shared::error::Result;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::base::{Base, Snapshot};

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
    /// New base links written from a legacy number.
    pub seeded: usize,
    /// Legacy tasks skipped: closed, never mirrored, or already in the base.
    pub skipped: usize,
}

/// Seed `territory`'s base from the legacy task JSON files in `legacy_dir`.
/// Idempotent and non-clobbering: an id already in the base is left untouched
/// (a prior sync's richer snapshot wins), so a re-run is safe. A malformed
/// `*.json` aborts before anything is written (the base is saved only after the
/// whole directory parses), so the operator fixes and re-runs cleanly.
pub fn adopt(legacy_dir: &Path, territory: &Path) -> Result<Summary> {
    let mut base = Base::load(territory)?;
    let mut summary = Summary::default();
    for entry in std::fs::read_dir(legacy_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue; // not a task file — ignore quietly
        }
        let task: LegacyTask = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        if seed_one(&mut base, &task) {
            summary.seeded += 1;
        } else {
            summary.skipped += 1;
        }
    }
    base.save(territory)?;
    Ok(summary)
}

/// Seed one task's link; `true` if a new base entry was written. Skips a closed
/// task (greenfield keeps no file for it — absence is closed, §9), a task that
/// was never mirrored to an issue, and an id the base already knows.
fn seed_one(base: &mut Base, task: &LegacyTask) -> bool {
    if task.status == "closed" {
        return false;
    }
    let Some(number) = task.issue_number() else {
        return false;
    };
    if base.get(&task.id).is_some() {
        return false;
    }
    base.set(
        &task.id,
        Snapshot { number, title: String::new(), body_hash: String::new(), state: "open".into() },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn seeds_a_live_task_from_its_legacy_number() {
        let legacy_dir = tempfile::tempdir().unwrap();
        let territory = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "bl-1a2b.json", &legacy("bl-1a2b", "open", 7));

        let summary = adopt(legacy_dir.path(), territory.path()).unwrap();
        assert_eq!(summary, Summary { seeded: 1, skipped: 0 });

        let snap = Base::load(territory.path()).unwrap().get("bl-1a2b").cloned().unwrap();
        assert_eq!(snap.number, 7);
        assert_eq!(snap.state, "open");
        assert_eq!(snap.title, "");
        assert_eq!(snap.body_hash, "");
    }

    #[test]
    fn skips_closed_never_mirrored_and_already_known() {
        let legacy_dir = tempfile::tempdir().unwrap();
        let territory = tempfile::tempdir().unwrap();
        // closed → absence is closed, no greenfield ball to link
        write(legacy_dir.path(), "bl-dead.json", &legacy("bl-dead", "closed", 1));
        // never mirrored → no issue number to seed
        write(legacy_dir.path(), "bl-bare.json", r#"{"id":"bl-bare","status":"open"}"#);
        // already known → a prior sync's snapshot must win
        write(legacy_dir.path(), "bl-old.json", &legacy("bl-old", "open", 9));

        let mut base = Base::default();
        base.set("bl-old", Snapshot { number: 9, title: "kept".into(), body_hash: "h".into(), state: "open".into() });
        base.save(territory.path()).unwrap();

        let summary = adopt(legacy_dir.path(), territory.path()).unwrap();
        assert_eq!(summary, Summary { seeded: 0, skipped: 3 });
        // the pre-existing richer snapshot is untouched
        assert_eq!(Base::load(territory.path()).unwrap().get("bl-old").unwrap().title, "kept");
    }

    #[test]
    fn ignores_non_json_files() {
        let legacy_dir = tempfile::tempdir().unwrap();
        let territory = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "README.md", "not a task");
        write(legacy_dir.path(), "bl-1.json", &legacy("bl-1", "open", 3));

        let summary = adopt(legacy_dir.path(), territory.path()).unwrap();
        assert_eq!(summary, Summary { seeded: 1, skipped: 0 });
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
    fn a_malformed_json_aborts_before_writing() {
        let legacy_dir = tempfile::tempdir().unwrap();
        let territory = tempfile::tempdir().unwrap();
        write(legacy_dir.path(), "bl-bad.json", "not json");
        assert!(adopt(legacy_dir.path(), territory.path()).is_err());
        // nothing was persisted
        assert!(Base::load(territory.path()).unwrap().entries.is_empty());
    }

    #[test]
    fn a_missing_legacy_dir_is_an_error() {
        let territory = tempfile::tempdir().unwrap();
        let missing = territory.path().join("nope");
        assert!(adopt(&missing, territory.path()).is_err());
    }
}
