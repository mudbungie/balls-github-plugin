//! Issues plugin config, git-tracked at `.balls/plugins/github-issues.json`.
//!
//! B1 ships the shared half only (`repo`, `api_base` via
//! `RepoConfig`). B2 adds the issues-specific fields
//! (`on_external_delete`, `close_mirror`, optional `target_label`,
//! per-event policy) and the projection-disjointness test.

use balls_github_shared::config_base::{load_json, RepoConfig};
use balls_github_shared::error::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct PluginConfig {
    #[serde(flatten)]
    pub base: RepoConfig,
}

impl PluginConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let cfg: PluginConfig = load_json(path)?;
        cfg.base.validate()?;
        Ok(cfg)
    }

    pub fn api_base(&self) -> &str {
        self.base.api_base()
    }

    pub fn repo(&self) -> &str {
        &self.base.repo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal() {
        let cfg: PluginConfig = serde_json::from_str(r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(cfg.repo(), "o/n");
        assert_eq!(cfg.api_base(), "https://api.github.com");
    }

    #[test]
    fn load_roundtrip_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.json");
        std::fs::write(&ok, r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(PluginConfig::load(&ok).unwrap().repo(), "o/n");

        let badrepo = dir.path().join("badrepo.json");
        std::fs::write(&badrepo, r#"{"repo":"noslash"}"#).unwrap();
        assert!(PluginConfig::load(&badrepo)
            .unwrap_err()
            .to_string()
            .contains("owner/name"));
    }
}
