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
/// ` [bl-…]` whose token satisfies [`is_id`] is removed from the bare title
/// and its id returned; otherwise the whole string is the bare title and the
/// id is `None`. The grammar gate matters: titles come from GitHub, UNTRUSTED,
/// and the parsed id flows into `tasks/<id>.md` path joins — a crafted marker
/// like `[bl-../../x]` must die here as plain title text, not become a read
/// outside the store (the bl-2d6d/938e75a0 traversal shape, bl-8a18).
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
    if !is_id(inner) {
        return (trimmed, None);
    }
    (trimmed[..open].trim_end(), Some(inner))
}

/// Whether `token` has the minted ball-id shape: `bl-` + 4–32 characters from
/// the core mint alphabet (`0123456789abcdef`, core `src/id.rs` `IdScheme` —
/// 4 today, 32 is headroom for a longer fixed width). This is the ONE id
/// grammar for every untrusted parse seam — the GitHub title marker here and
/// `bl` stdout in `crate::shellback::extract_id` — so no separator (`/`, `.`,
/// `-`, `_`) ever survives into a downstream path join.
#[must_use]
pub fn is_id(token: &str) -> bool {
    token.strip_prefix("bl-").is_some_and(|hex| {
        (4..=32).contains(&hex.len())
            && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
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
        assert_eq!(append("Fix [bl-01d0]", "bl-2ef1"), "Fix [bl-2ef1]");
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

    #[test]
    fn strip_rejects_a_hostile_traversal_marker() {
        // GitHub-controlled title; the "id" must not reach a path join.
        let title = "Fix the bug [bl-../../x]";
        assert_eq!(strip(title), (title, None));
        assert_eq!(strip("Fix [bl-tasks/evil]"), ("Fix [bl-tasks/evil]", None));
    }

    #[test]
    fn strip_rejects_a_non_minted_alphabet_or_width() {
        assert_eq!(strip("Fix [bl-1A2B]"), ("Fix [bl-1A2B]", None)); // uppercase
        assert_eq!(strip("Fix [bl-xyzw]"), ("Fix [bl-xyzw]", None)); // non-hex
        assert_eq!(strip("Fix [bl-1a2]"), ("Fix [bl-1a2]", None)); // too short
        assert_eq!(strip("Fix [bl-]"), ("Fix [bl-]", None)); // empty suffix
    }

    #[test]
    fn is_id_accepts_exactly_the_minted_shape() {
        assert!(is_id("bl-1a2b"));
        assert!(is_id(&format!("bl-{}", "0".repeat(32)))); // max headroom
        assert!(!is_id(&format!("bl-{}", "0".repeat(33))));
        assert!(!is_id("bl-../../x"));
        assert!(!is_id("xl-1a2b"));
    }
}
