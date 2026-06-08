//! The reconciliation base (bl-613d) — the ONE piece of state the mirror
//! genuinely owns: a per-issue snapshot of the last point balls and GitHub
//! agreed on.
//!
//! Neither side holds this fact (GitHub has *current*, the ball has *current*),
//! so a three-way merge needs it as the common ancestor. It also subsumes two
//! other mechanisms the old protocol carried as separate fields:
//! - **idempotency / loop-avoidance** — push compares the ball to this base and
//!   no-ops when equal, so a pull that wrote `bl update` does not bounce back
//!   out to GitHub (the base was advanced to GH-now during the pull).
//! - **the number cache** — the join's source of truth is the `[bl-xxxx]` title
//!   marker (`crate::marker`), but caching the number here lets the push path
//!   PATCH without listing the whole repo.
//!
//! It lives in plugin territory (`crate::territory`), JSON, machine-local. A
//! fresh clone with no base degrades gracefully: the first sync adopts GH-now
//! as the base (a resync, no data loss), because the marker still recovers the
//! join.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// The last-agreed state of one mirrored issue, keyed by ball id in [`Base`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The GitHub issue number — the join cache (marker is the SSOT).
    pub number: u64,
    /// The bare (marker-stripped) title both sides last agreed on.
    pub title: String,
    /// Hash of the body both sides last agreed on (`crate::content::body_hash`).
    pub body_hash: String,
    /// The GitHub issue state (`open`/`closed`) last observed.
    pub state: String,
}

/// The whole base: ball id → [`Snapshot`]. Absent file = empty (first run).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Base {
    pub entries: BTreeMap<String, Snapshot>,
}

fn base_path(dir: &Path) -> PathBuf {
    dir.join("base.json")
}

impl Base {
    /// Load the base from `dir/base.json`. A missing file is an empty base (the
    /// first-run / fresh-clone path — the general case with no entries, not a
    /// special case). A malformed file is an error.
    pub fn load(dir: &Path) -> io::Result<Base> {
        match std::fs::read_to_string(base_path(dir)) {
            Ok(data) => serde_json::from_str(&data).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Base::default()),
            Err(e) => Err(e),
        }
    }

    /// Persist the base to `dir/base.json`, creating `dir` if needed.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let data = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(base_path(dir), data)
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Snapshot> {
        self.entries.get(id)
    }

    pub fn set(&mut self, id: &str, snap: Snapshot) {
        self.entries.insert(id.to_string(), snap);
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }

    /// Reverse lookup: the ball id mapped to GitHub issue `number`, if any. The
    /// fallback join when a GH title has lost its `[bl-xxxx]` marker but the base
    /// still remembers the link.
    #[must_use]
    pub fn id_for_number(&self, number: u64) -> Option<&str> {
        self.entries
            .iter()
            .find(|(_, s)| s.number == number)
            .map(|(id, _)| id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(n: u64) -> Snapshot {
        Snapshot {
            number: n,
            title: "T".into(),
            body_hash: "h".into(),
            state: "open".into(),
        }
    }

    #[test]
    fn missing_file_is_an_empty_base() {
        let dir = tempfile::tempdir().unwrap();
        let base = Base::load(dir.path()).unwrap();
        assert!(base.entries.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut base = Base::default();
        base.set("bl-1a2b", snap(7));
        base.save(dir.path()).unwrap();

        let loaded = Base::load(dir.path()).unwrap();
        assert_eq!(loaded.get("bl-1a2b"), Some(&snap(7)));
    }

    #[test]
    fn save_creates_a_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        Base::default().save(&nested).unwrap();
        assert!(base_path(&nested).exists());
    }

    #[test]
    fn set_get_remove() {
        let mut base = Base::default();
        assert!(base.get("bl-x").is_none());
        base.set("bl-x", snap(1));
        assert_eq!(base.get("bl-x").unwrap().number, 1);
        base.remove("bl-x");
        assert!(base.get("bl-x").is_none());
    }

    #[test]
    fn reverse_lookup_by_number() {
        let mut base = Base::default();
        base.set("bl-a", snap(7));
        base.set("bl-b", snap(9));
        assert_eq!(base.id_for_number(9), Some("bl-b"));
        assert_eq!(base.id_for_number(404), None);
    }

    #[test]
    fn malformed_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(base_path(dir.path()), "not json").unwrap();
        assert!(Base::load(dir.path()).is_err());
    }

    #[test]
    fn a_non_notfound_io_error_propagates() {
        // base.json is a directory → read fails with a non-NotFound error.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(base_path(dir.path())).unwrap();
        assert!(Base::load(dir.path()).is_err());
    }
}
