//! Shared config base: the repo/api_base half of every GitHub plugin's
//! config file. Plugins compose this via `#[serde(flatten)] base:
//! RepoConfig`, then add their own per-plugin fields (the forge
//! plugin adds `target_branch`; the issues plugin adds
//! `on_external_delete`, `close_mirror`, label filters, …).
//!
//! `load_json` is the only file-reading entry point in the shared
//! crate. It maps I/O and parse errors into `PluginError::Config` with
//! the path attached so the error message is useful at the CLI.

use crate::error::{PluginError, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct RepoConfig {
    /// `owner/name`, e.g. `mudbungie/balls`.
    pub repo: String,
    /// API root. Override for GitHub Enterprise. Defaults to public GH.
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_api_base() -> String {
    "https://api.github.com".to_string()
}

impl RepoConfig {
    /// Splits `repo` into `(owner, name)`, rejecting empty or nested forms.
    pub fn owner_name(&self) -> Option<(&str, &str)> {
        let (owner, name) = self.repo.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some((owner, name))
    }

    pub fn api_base(&self) -> &str {
        self.api_base.trim_end_matches('/')
    }

    /// Fail loudly if `repo` is malformed. Called by each plugin's
    /// `load` after deserialization so the diagnostic happens once,
    /// against the user's actual file.
    pub fn validate(&self) -> Result<()> {
        if self.owner_name().is_none() {
            return Err(PluginError::Config(format!(
                "repo must be \"owner/name\", got {:?}",
                self.repo
            )));
        }
        Ok(())
    }
}

/// Read JSON from `path` and deserialize. The shared point of entry
/// for plugin config loading; per-plugin extensions deserialize via
/// this then call their own validation pass.
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| PluginError::Config(format!("{}: {}", path.display(), e)))?;
    serde_json::from_str(&data)
        .map_err(|e| PluginError::Config(format!("{}: {}", path.display(), e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal() {
        let cfg: RepoConfig = serde_json::from_str(r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(cfg.owner_name(), Some(("o", "n")));
        assert_eq!(cfg.api_base(), "https://api.github.com");
    }

    #[test]
    fn api_base_override_trimmed() {
        let cfg: RepoConfig = serde_json::from_str(
            r#"{"repo":"a/b","api_base":"https://ghe.x/api/v3/"}"#,
        )
        .unwrap();
        assert_eq!(cfg.api_base(), "https://ghe.x/api/v3");
    }

    #[test]
    fn owner_name_rejects_bad_forms() {
        let bad = |r: &str| {
            serde_json::from_str::<RepoConfig>(&format!(r#"{{"repo":{r:?}}}"#))
                .unwrap()
                .owner_name()
                .is_none()
        };
        assert!(bad("noslash"));
        assert!(bad("/n"));
        assert!(bad("o/"));
        assert!(bad("a/b/c"));
    }

    #[test]
    fn validate_rejects_malformed() {
        let cfg: RepoConfig = serde_json::from_str(r#"{"repo":"noslash"}"#).unwrap();
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("owner/name"));
    }

    #[test]
    fn validate_accepts_well_formed() {
        let cfg: RepoConfig = serde_json::from_str(r#"{"repo":"o/n"}"#).unwrap();
        cfg.validate().unwrap();
    }

    #[test]
    fn load_json_ok_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.json");
        std::fs::write(&ok, r#"{"repo":"o/n"}"#).unwrap();
        let cfg: RepoConfig = load_json(&ok).unwrap();
        assert_eq!(cfg.repo, "o/n");

        let missing = dir.path().join("missing.json");
        let err = load_json::<RepoConfig>(&missing).unwrap_err().to_string();
        assert!(err.contains("missing.json"));

        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "not json").unwrap();
        assert!(load_json::<RepoConfig>(&bad).is_err());
    }
}
