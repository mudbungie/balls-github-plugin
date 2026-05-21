//! Title/body content-mirror helpers for the pull side (bl-4918).
//!
//! Split out of `pull_emit.rs` so the close-mirror / auto-create /
//! external-delete logic in that file stays focused. The helpers here
//! are about answering one question per field: *who moved?* — given
//! the GH-side value, the balls-side value, and the `last_synced_*`
//! projection that records what we last pushed.
//!
//! The merge contract is the asymmetric one from `merge.rs`:
//!
//! | gh_moved | balls_moved | result                                  |
//! |----------|-------------|-----------------------------------------|
//! | no       | *           | leave balls (we're already converged or |
//! |          |             | balls moved alone — push will sync it)  |
//! | yes      | no          | mirror GH to balls                      |
//! | yes      | yes         | conflict: leave balls, emit add_note;   |
//! |          |             | last_synced_* advances to GH's value so |
//! |          |             | the next push (balls-wins per merge.rs) |
//! |          |             | resolves it.                            |
//!
//! Body comparison goes through a bounded hash (FNV-1a-64 → 16-char
//! hex) so the projection stays constant-size per SPEC 6's "no new
//! state stores" intent.

use crate::pull::GhIssue;
use balls_github_shared::types::Task;

/// FNV-1a 64-bit hex digest. Deterministic across versions (the
/// constants are part of the FNV spec), dependency-free, and used
/// only for change detection — collision-resistance is not required:
/// the worst-case outcome of a hash collision is "we think balls
/// didn't move when it did", which causes a benign GH-wins sync that
/// the next push will undo per the balls-wins merge contract.
pub fn body_hash(s: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut h: u64 = FNV_OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:016x}")
}

/// What this title is going to look like on GH after the next push.
/// Includes the `[bl-xxxx]` marker because `last_synced_title` is
/// stored with the marker too — a like-for-like comparison.
pub fn pushed_title(task: &Task) -> String {
    format!("{} [{}]", task.title, task.id)
}

/// Strip the trailing `[bl-<id>]` marker the push side appends. Used
/// when mirroring a GH-side title back into `task.title` (which is
/// stored without the marker — balls owns the marker). A title GH
/// edited that drops or mangles the marker still mirrors back as-is.
pub fn strip_marker(title: &str, task_id: &str) -> String {
    let marker = format!(" [{task_id}]");
    title
        .strip_suffix(&marker)
        .map_or_else(|| title.to_string(), str::to_string)
}

#[derive(Debug, PartialEq, Eq)]
pub enum FieldDecision {
    /// Nothing to do: either both sides match or only balls moved.
    Noop,
    /// GH moved alone; mirror its value to balls.
    Mirror,
    /// Both sides moved; leave balls, emit a note. `last_synced_*`
    /// advances on emit so we don't re-emit the conflict every poll.
    Conflict,
}

pub fn decide_title(issue: &GhIssue, task: &Task, last_synced: &str) -> FieldDecision {
    let gh_moved = issue.title != last_synced;
    let balls_moved = pushed_title(task) != last_synced;
    decision(gh_moved, balls_moved)
}

pub fn decide_body(issue: &GhIssue, task: &Task, last_hash: &str) -> FieldDecision {
    let gh_body = issue.body.as_deref().unwrap_or("");
    let gh_moved = body_hash(gh_body) != last_hash;
    let balls_moved = body_hash(&task.description) != last_hash;
    decision(gh_moved, balls_moved)
}

fn decision(gh_moved: bool, balls_moved: bool) -> FieldDecision {
    match (gh_moved, balls_moved) {
        (false, _) => FieldDecision::Noop,
        (true, false) => FieldDecision::Mirror,
        (true, true) => FieldDecision::Conflict,
    }
}

#[cfg(test)]
#[path = "pull_content_tests.rs"]
mod tests;
