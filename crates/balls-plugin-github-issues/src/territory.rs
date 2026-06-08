//! Plugin territory (§1/§6) — the one subtree this plugin owns.
//!
//! Under the no-return-channel protocol the plugin keeps its private state
//! OFF the ball (bl-613d): the GitHub token and the reconciliation base
//! (`crate::base`) live in `$XDG_STATE_HOME/balls/plugins/github-issues/`,
//! project-scoped by mirroring the invocation path. balls never reads this
//! subtree; the layout below is the plugin's own choice, so uninstalling the
//! plugin is `rm -rf` of this directory with zero ball or core edits.
//!
//! No env reads happen below the edge: the binary resolves `HOME`/
//! `XDG_STATE_HOME` once in `main` and hands them here, mirroring balls' own
//! "no env reads in the lib" rule.

use std::path::{Path, PathBuf};

/// The XDG bases the territory derives from, resolved once at the process edge.
#[derive(Debug, Clone)]
pub struct Xdg {
    pub home: PathBuf,
    pub state_home: Option<PathBuf>,
}

impl Xdg {
    /// `$XDG_STATE_HOME` if set, else the XDG default `$HOME/.local/state`.
    fn state_root(&self) -> PathBuf {
        self.state_home
            .clone()
            .unwrap_or_else(|| self.home.join(".local").join("state"))
    }

    /// This plugin's territory for `invocation_path`:
    /// `…/balls/plugins/github-issues/<invocation-mirrored>/`. The invocation
    /// path is mirrored (leading `/` stripped) for a readable, collision-free
    /// per-project key — the same convention the delivery plugin uses (§1).
    #[must_use]
    pub fn territory(&self, invocation_path: &str) -> PathBuf {
        self.state_root()
            .join("balls")
            .join("plugins")
            .join("github-issues")
            .join(mirror(invocation_path))
    }
}

/// Strip a leading `/` so an absolute invocation path becomes one relative
/// component group under the territory root (never escaping it).
fn mirror(path: &str) -> &Path {
    Path::new(path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xdg(home: &str, state: Option<&str>) -> Xdg {
        Xdg {
            home: PathBuf::from(home),
            state_home: state.map(PathBuf::from),
        }
    }

    #[test]
    fn territory_uses_state_home_when_set() {
        let x = xdg("/home/me", Some("/var/state"));
        assert_eq!(
            x.territory("/home/me/dev/proj"),
            PathBuf::from("/var/state/balls/plugins/github-issues/home/me/dev/proj"),
        );
    }

    #[test]
    fn territory_falls_back_to_home_default() {
        let x = xdg("/home/me", None);
        assert_eq!(
            x.territory("/proj"),
            PathBuf::from("/home/me/.local/state/balls/plugins/github-issues/proj"),
        );
    }

    #[test]
    fn mirror_strips_only_the_leading_slash() {
        assert_eq!(mirror("/a/b"), Path::new("a/b"));
        assert_eq!(mirror("a/b"), Path::new("a/b"));
    }
}
