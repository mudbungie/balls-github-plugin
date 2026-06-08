//! The `[bl-xxxx]` issue-title marker — the SINGLE source of truth for the
//! task↔issue join (bl-613d).
//!
//! Under the §6/§7 no-return-channel protocol the plugin stores no issue number
//! on the ball; the durable link lives where GitHub is authoritative — appended
//! to the issue title as `… [bl-xxxx]`. Push writes it; pull reads it back to
//! recover which ball an external issue belongs to. The reconciliation base
//! (`crate::base`) caches the number for the push path, but the marker is what
//! survives a fresh clone with no local state.

/// Append `[id]` to `title`, idempotently: a title that already carries this
/// exact marker is returned unchanged, and a title carrying a *different*
/// `[bl-…]` marker has it replaced. So pushing the same task twice never
/// double-stamps, and a renamed ball re-stamps cleanly.
#[must_use]
pub fn append(title: &str, id: &str) -> String {
    let bare = strip(title).0;
    if bare.is_empty() {
        format!("[{id}]")
    } else {
        format!("{bare} [{id}]")
    }
}

/// Split a possibly-marked title into `(bare_title, marked_id)`. A trailing
/// ` [bl-…]` (the marker grammar: a bracketed token starting `bl-`) is removed
/// from the bare title and its id returned; otherwise the whole string is the
/// bare title and the id is `None`.
#[must_use]
pub fn strip(title: &str) -> (&str, Option<&str>) {
    let trimmed = title.trim_end();
    let Some(open) = trimmed.rfind('[') else {
        return (trimmed, None);
    };
    if !trimmed.ends_with(']') {
        return (trimmed, None);
    }
    let inner = &trimmed[open + 1..trimmed.len() - 1];
    if !inner.starts_with("bl-") || inner.contains([' ', '[', ']']) {
        return (trimmed, None);
    }
    (trimmed[..open].trim_end(), Some(inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_to_bare_title() {
        assert_eq!(append("Fix the bug", "bl-1a2b"), "Fix the bug [bl-1a2b]");
    }

    #[test]
    fn append_is_idempotent_on_same_id() {
        assert_eq!(append("Fix [bl-1a2b]", "bl-1a2b"), "Fix [bl-1a2b]");
    }

    #[test]
    fn append_replaces_a_different_marker() {
        assert_eq!(append("Fix [bl-old0]", "bl-new1"), "Fix [bl-new1]");
    }

    #[test]
    fn append_to_empty_bare_is_just_the_marker() {
        assert_eq!(append("[bl-1a2b]", "bl-1a2b"), "[bl-1a2b]");
        assert_eq!(append("", "bl-1a2b"), "[bl-1a2b]");
    }

    #[test]
    fn strip_reads_the_marker() {
        assert_eq!(strip("Fix the bug [bl-1a2b]"), ("Fix the bug", Some("bl-1a2b")));
    }

    #[test]
    fn strip_unmarked_title() {
        assert_eq!(strip("Fix the bug"), ("Fix the bug", None));
    }

    #[test]
    fn strip_ignores_non_marker_brackets() {
        assert_eq!(strip("Fix [draft]"), ("Fix [draft]", None));
        assert_eq!(strip("Fix [bl with space]"), ("Fix [bl with space]", None));
        assert_eq!(strip("Fix the bug ["), ("Fix the bug [", None));
    }

    #[test]
    fn strip_tolerates_trailing_space() {
        assert_eq!(strip("Fix [bl-1a2b]  "), ("Fix", Some("bl-1a2b")));
    }
}
