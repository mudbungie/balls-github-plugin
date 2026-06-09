//! Reading the STORE checkout on the pull side (bl-613d).
//!
//! During `sync` the plugin's cwd is the live store checkout (§13) — `tasks/` is
//! right there. The plugin READS it directly (the balls-side title + body for a
//! three-way merge, and the set of live ids for the delete-sweep) but never
//! WRITES it: every mutation goes through a shelled `bl` verb (`crate::shellback`)
//! so the lifecycle and its hooks still run. This module is the read half only.
//!
//! A ball is `tasks/<id>.md`: a `+++`-fenced TOML frontmatter block then a
//! markdown body (§3). We need the `title` (parsed from the frontmatter) and the
//! body (everything after the closing fence).

use serde::Deserialize;
use std::io;
use std::path::{Path, PathBuf};

/// The balls-side view of one ball, for merge comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ball {
    pub title: String,
    pub body: String,
}

#[derive(Deserialize)]
struct Frontmatter {
    title: String,
}

fn tasks_dir(store: &Path) -> PathBuf {
    store.join("tasks")
}

/// Parse a `tasks/<id>.md` file body into a [`Ball`]. `None` if the fences are
/// missing or the frontmatter has no `title` — a malformed ball is skipped, not
/// fatal (the sweep keeps going).
#[must_use]
pub fn parse(content: &str) -> Option<Ball> {
    let rest = content.strip_prefix("+++\n")?;
    let (frontmatter, body) = match rest.split_once("\n+++\n") {
        Some((fm, body)) => (fm, body),
        None => (rest.strip_suffix("\n+++")?, ""),
    };
    let fm: Frontmatter = toml::from_str(frontmatter).ok()?;
    Some(Ball { title: fm.title, body: body.to_string() })
}

/// Read `<store>/tasks/<id>.md`. `Ok(None)` if the file is absent (the task is
/// closed/dropped) or unparseable.
pub fn read_ball(store: &Path, id: &str) -> io::Result<Option<Ball>> {
    let path = tasks_dir(store).join(format!("{id}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(parse(&content)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether `<store>/tasks/<id>.md` exists (the task is live).
pub fn is_live(store: &Path, id: &str) -> bool {
    tasks_dir(store).join(format!("{id}.md")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_ball() {
        let b = parse("+++\ntitle = \"Hello\"\ncreated = 1\n+++\nbody text\n").unwrap();
        assert_eq!(b.title, "Hello");
        assert_eq!(b.body, "body text\n");
    }

    #[test]
    fn parse_empty_body() {
        let b = parse("+++\ntitle = \"T\"\n+++").unwrap();
        assert_eq!(b.title, "T");
        assert_eq!(b.body, "");
    }

    #[test]
    fn parse_title_with_quotes_and_marker() {
        let b = parse("+++\ntitle = \"Fix \\\"x\\\" [bl-1]\"\n+++\n").unwrap();
        assert_eq!(b.title, "Fix \"x\" [bl-1]");
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(parse("no fences").is_none());
        assert!(parse("+++\nnot toml = = =\n+++\n").is_none());
        assert!(parse("+++\ncreated = 1\n+++\n").is_none()); // no title
        assert!(parse("+++\ntitle=\"x\"").is_none()); // unterminated
    }

    #[test]
    fn read_ball_present_absent() {
        let dir = tempfile::tempdir().unwrap();
        let tasks = tasks_dir(dir.path());
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(tasks.join("bl-1.md"), "+++\ntitle = \"T\"\n+++\nbody\n").unwrap();

        assert_eq!(read_ball(dir.path(), "bl-1").unwrap().unwrap().title, "T");
        assert!(read_ball(dir.path(), "bl-missing").unwrap().is_none());
        assert!(is_live(dir.path(), "bl-1"));
        assert!(!is_live(dir.path(), "bl-missing"));
    }

    #[test]
    fn a_non_notfound_io_error_propagates() {
        // tasks/<id>.md is a directory → read fails with a non-NotFound error.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tasks_dir(dir.path()).join("bl-d.md")).unwrap();
        assert!(read_ball(dir.path(), "bl-d").is_err());
    }
}
