//! The participant's merge contract for the issues plugin.
//!
//! Two layers, per SPEC-lifecycle-sync-participants §5 and §8 plus
//! B5's acceptance criteria:
//!
//! 1. **Projection overlap (`external.github-issues.*`)**: both sides
//!    moved the foreign key under the plugin's own namespace. The
//!    plugin owns this projection authoritatively; the merge picks
//!    the plugin's view (which is GH's view, since this projection
//!    mirrors GH). This is the default merge for an owned
//!    projection per SPEC §5.
//!
//! 2. **Status overlap (gated by `close_mirror`)**: the plugin
//!    claims write-authority on `task.status` for the close
//!    transition *only*. The merge asymmetry:
//!    - GH closed + balls open  → balls.status = closed (mirror).
//!    - GH open   + balls anything → balls wins (workflow truth).
//!    - close_mirror=Off → balls always wins, GH never owns status.
//!
//! These functions are not yet wired to balls-core's native
//! `describe` handshake (the issues plugin is a legacy participant
//! today). Exporting them here locks the contract in code that
//! exercises the documented behavior under unit tests; native-
//! protocol wiring is a deferred follow-up tracked alongside
//! bl-4918.

// The functions in this module are documentation-as-code: they
// pin the merge contract under unit tests but aren't yet wired to
// balls-core's native describe handshake (the issues plugin is a
// legacy participant today). `#[allow(dead_code)]` is honest until
// the native-protocol wiring lands; removing the allow without
// adding a call site is a regression.
#![allow(dead_code)]

use crate::config::{CloseMirror, PROJECTION_PREFIX};
use serde_json::Value;

/// Merge two views of `external.github-issues.*`. By the projection
/// authority rule, the plugin's (remote/GH) view wins outright.
/// Local state is overwritten when remote is present; absent
/// remote keeps local (idempotent on a no-op sync where the plugin
/// emits no projection update).
pub fn merge_projection(local: Option<&Value>, remote: Option<&Value>) -> Option<Value> {
    match (local, remote) {
        (_, Some(r)) => Some(r.clone()),
        (Some(l), None) => Some(l.clone()),
        (None, None) => None,
    }
}

/// Apply the status-merge asymmetry. Returns the status balls
/// should hold after the merge resolves.
pub fn merge_status(
    local_status: &str,
    gh_state: &str,
    close_mirror: CloseMirror,
) -> String {
    // close_mirror=Off makes GH never authoritative on status.
    if close_mirror == CloseMirror::Off {
        return local_status.to_string();
    }
    // GH closed beats balls anything-not-closed.
    if gh_state == "closed" && local_status != "closed" {
        return "closed".to_string();
    }
    // Otherwise balls wins (workflow truth; GH open never re-opens
    // a closed balls task by itself).
    local_status.to_string()
}

/// The projection prefix this plugin owns. Exposed here (rather
/// than only as a const in config.rs) so callers reading the merge
/// contract can reach it from a documented entry point.
pub fn owned_projection_prefix() -> &'static str {
    PROJECTION_PREFIX
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
