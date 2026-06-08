//! The forge plugin's XDG state territory (§1):
//! `$XDG_STATE_HOME/balls/plugins/<name>/`. It holds two kinds of local,
//! gitignored state the plugin owns:
//!
//! - **`auth/`** — the secret GitHub token (mode 0600, via the shared `auth`
//!   module). Per-machine, not per-project.
//! - **`by-project/<invocation>/gates/<parent>`** — the `parent → gate-child`
//!   link. §7 gives the plugin no return channel to store this on the ball, and
//!   storing it as a frontmatter key would be eaten by `close`'s file-deletion
//!   seal (the §11 `delivered_in` lesson), so the plugin keeps it in its OWN
//!   territory, keyed by invocation path so two projects never collide.

use balls_github_shared::error::Result;
use std::path::{Path, PathBuf};

/// The territory rooted at one `(state_home, plugin-name, invocation-path)`.
pub struct Territory {
    root: PathBuf,
    gates: PathBuf,
}

impl Territory {
    /// Locate the territory. `invocation` is MIRRORED (leading `/` stripped) so
    /// the path is inspectable and never `%`-encoded — pure data, but mirroring
    /// matches the delivery plugin's own territory convention (§1/§11).
    #[must_use]
    pub fn new(state_home: &Path, name: &str, invocation: &str) -> Self {
        let root = state_home.join("balls").join("plugins").join(name);
        let gates = root.join("by-project").join(invocation.trim_start_matches('/')).join("gates");
        Self { root, gates }
    }

    /// The auth dir (`auth/`) — where the shared `auth` module reads/writes the
    /// token. Independent of invocation: one token per machine.
    #[must_use]
    pub fn auth_dir(&self) -> PathBuf {
        self.root.join("auth")
    }

    fn gate_file(&self, parent: &str) -> PathBuf {
        self.gates.join(parent)
    }

    /// Record `parent → gate`.
    pub fn remember_gate(&self, parent: &str, gate: &str) -> Result<()> {
        std::fs::create_dir_all(&self.gates)?;
        std::fs::write(self.gate_file(parent), gate)?;
        Ok(())
    }

    /// Read back `parent`'s gate id (`None` if none recorded).
    pub fn recall_gate(&self, parent: &str) -> Result<Option<String>> {
        match std::fs::read_to_string(self.gate_file(parent)) {
            Ok(s) => Ok(Some(s.trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Forget `parent → gate` (idempotent: a missing link is a clean no-op).
    pub fn forget_gate(&self, parent: &str) -> Result<()> {
        match std::fs::remove_file(self.gate_file(parent)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Every recorded `(parent, gate)` pair — `sync` scans these for merged PRs.
    /// An absent gates dir (nothing ever gated here) is an empty list.
    pub fn pending_gates(&self) -> Result<Vec<(String, String)>> {
        let read = match std::fs::read_dir(&self.gates) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in read {
            let path = entry?.path();
            // Every dir entry has a final component, and we only ever write
            // ascii-id files here, so the name is always present and lossless.
            let parent = path.file_name().expect("dir entry has a name").to_string_lossy().into_owned();
            let gate = std::fs::read_to_string(&path)?.trim().to_string();
            out.push((parent, gate));
        }
        out.sort();
        Ok(out)
    }
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod tests;
