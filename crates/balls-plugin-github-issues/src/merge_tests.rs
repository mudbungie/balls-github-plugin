//! Conformance tests for the issues plugin's merge contract. These
//! tests pin the rules documented in `merge.rs` against drift; B5's
//! acceptance criteria each map to at least one test below.

use super::*;
use crate::config::FORGE_PROJECTION_PREFIX;
use serde_json::json;

#[test]
fn projection_prefix_is_disjoint_from_forge() {
    // SPEC-lifecycle-sync-participants §3: two participants on the
    // same Task must own disjoint projection namespaces. The
    // disjointness is structural — neither prefix is a substring of
    // the other.
    let owned = owned_projection_prefix();
    assert_ne!(owned, FORGE_PROJECTION_PREFIX);
    assert!(!owned.starts_with(FORGE_PROJECTION_PREFIX));
    assert!(!FORGE_PROJECTION_PREFIX.starts_with(owned));
}

#[test]
fn merge_projection_picks_remote_when_present() {
    // Plugin's view wins on its owned projection.
    let local = json!({"issue":{"number":1,"state":"open"}});
    let remote = json!({"issue":{"number":1,"state":"closed"}});
    let merged = merge_projection(Some(&local), Some(&remote)).unwrap();
    assert_eq!(merged["issue"]["state"], "closed");
}

#[test]
fn merge_projection_keeps_local_when_remote_absent() {
    // A no-op sync doesn't tear the projection down — idempotency.
    let local = json!({"issue":{"number":1,"state":"open"}});
    let merged = merge_projection(Some(&local), None).unwrap();
    assert_eq!(merged["issue"]["state"], "open");
}

#[test]
fn merge_projection_returns_none_when_both_absent() {
    assert!(merge_projection(None, None).is_none());
}

#[test]
fn status_merge_authoritative_close() {
    // GH closed + balls open -> balls closes (the documented
    // mirror direction).
    assert_eq!(
        merge_status("open", "closed", CloseMirror::Authoritative),
        "closed"
    );
    assert_eq!(
        merge_status("in_progress", "closed", CloseMirror::Authoritative),
        "closed"
    );
}

#[test]
fn status_merge_already_closed_is_idempotent() {
    assert_eq!(
        merge_status("closed", "closed", CloseMirror::Authoritative),
        "closed"
    );
}

#[test]
fn status_merge_gh_open_never_reopens_balls() {
    // The asymmetry: GH open does not authoritatively re-open a
    // balls task. balls owns workflow direction.
    assert_eq!(
        merge_status("closed", "open", CloseMirror::Authoritative),
        "closed"
    );
    assert_eq!(
        merge_status("in_progress", "open", CloseMirror::Authoritative),
        "in_progress"
    );
}

#[test]
fn status_merge_close_mirror_off_protects_balls_status() {
    // close_mirror=Off: GH never owns status, no matter what.
    assert_eq!(merge_status("open", "closed", CloseMirror::Off), "open");
    assert_eq!(
        merge_status("in_progress", "closed", CloseMirror::Off),
        "in_progress"
    );
}

#[test]
fn status_merge_best_effort_still_mirrors_close() {
    // BestEffort is a *policy* (downstream behavior on failure); the
    // mirror direction is the same as Authoritative.
    assert_eq!(
        merge_status("open", "closed", CloseMirror::BestEffort),
        "closed"
    );
}
