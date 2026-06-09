//! Content reconciliation primitives (bl-613d).
//!
//! The greenfield verb surface (`bl update`) cannot set a ball's `title` or
//! `body` — only `-t`/`-p`/`key=value` and create/close. That ENFORCES the
//! plugin's locked authority model: *balls is authoritative for content; GitHub
//! owns only the close transition inward.* So there is no inward title/body
//! mirror to compute — when GitHub and balls disagree on content, balls wins and
//! the difference is re-pushed OUT to GitHub. What remains here is therefore
//! minimal:
//! - [`body_hash`] — change detection for push idempotency / loop-avoidance.
//! - [`differs`] — does GitHub's content drift from balls' (→ re-push out)?
//! - [`mirror_close`] — the one inward exception: a GH close closes the task.

use crate::config::CloseMirror;

/// FNV-1a 64-bit hex digest. Deterministic (the constants are the FNV spec),
/// dependency-free, change-detection only — a collision's worst case is a benign
/// redundant PATCH the next reconcile settles.
#[must_use]
pub fn body_hash(s: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x100_0000_01b3;
    let mut h: u64 = FNV_OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:016x}")
}

/// Does GitHub's content differ from balls'? Compares the BARE titles
/// (marker-stripped — balls stores no marker) and the body hashes. `true` means
/// the issue should be re-PATCHed to match balls (balls wins, except close).
#[must_use]
pub fn differs(gh_bare_title: &str, gh_body: &str, balls_title: &str, balls_body: &str) -> bool {
    gh_bare_title != balls_title || body_hash(gh_body) != body_hash(balls_body)
}

/// The one inward content exception: should a GH-closed issue close the live
/// balls task? `Off` → never; otherwise GH-closed beats balls-live. A GH-open
/// issue never re-opens a task by itself (that direction isn't even expressible).
#[must_use]
pub fn mirror_close(gh_closed: bool, balls_live: bool, policy: CloseMirror) -> bool {
    policy != CloseMirror::Off && gh_closed && balls_live
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_distinguishes() {
        assert_eq!(body_hash(""), body_hash(""));
        assert_eq!(body_hash("hello").len(), 16);
        assert_ne!(body_hash("a"), body_hash("b"));
    }

    #[test]
    fn differs_on_title_or_body() {
        assert!(!differs("T", "b", "T", "b")); // identical
        assert!(differs("G", "b", "T", "b")); // title drift
        assert!(differs("T", "x", "T", "b")); // body drift
        assert!(differs("G", "x", "T", "b")); // both
    }

    #[test]
    fn close_mirror_asymmetry() {
        use CloseMirror::*;
        assert!(mirror_close(true, true, Authoritative));
        assert!(mirror_close(true, true, BestEffort));
        assert!(!mirror_close(true, true, Off)); // policy off
        assert!(!mirror_close(false, true, Authoritative)); // gh not closed
        assert!(!mirror_close(true, false, Authoritative)); // task already gone
    }
}
