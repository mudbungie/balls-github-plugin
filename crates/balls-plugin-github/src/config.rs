//! Forge plugin config, committed on the landing at
//! `config/plugins/<plugin-name>.json` (the name is `BALLS_PLUGIN_NAME`,
//! §6 install bundle). The shared half (`repo`, `api_base`) flattens in
//! from `balls_github_shared::config_base::RepoConfig`; the forge-only
//! field `target_branch` lives here.

use balls_github_shared::config_base::{load_json, RepoConfig};
use balls_github_shared::error::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct PluginConfig {
    #[serde(flatten)]
    pub base: RepoConfig,
    /// Default PR base. Per-task `target_branch` overrides it. A forge PR
    /// needs a base somewhere; `None` here and on the task is an error at
    /// `close.pre`, never silently `main`.
    #[serde(default)]
    pub target_branch: Option<String>,
}

impl PluginConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let cfg: PluginConfig = load_json(path)?;
        cfg.base.validate()?;
        Ok(cfg)
    }

    pub fn owner_name(&self) -> Option<(&str, &str)> {
        self.base.owner_name()
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
        assert_eq!(cfg.owner_name(), Some(("o", "n")));
        assert!(cfg.target_branch.is_none());
        assert_eq!(cfg.api_base(), "https://api.github.com");
        assert_eq!(cfg.repo(), "o/n");
    }

    #[test]
    fn deserialize_full() {
        let cfg: PluginConfig = serde_json::from_str(
            r#"{"repo":"a/b","target_branch":"develop","api_base":"https://ghe.x/api/v3/"}"#,
        )
        .unwrap();
        assert_eq!(cfg.target_branch.as_deref(), Some("develop"));
        assert_eq!(cfg.api_base(), "https://ghe.x/api/v3");
    }

    #[test]
    fn load_roundtrip_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.json");
        std::fs::write(&ok, r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(PluginConfig::load(&ok).unwrap().repo(), "o/n");

        assert!(PluginConfig::load(&dir.path().join("missing.json"))
            .unwrap_err()
            .to_string()
            .contains("missing.json"));

        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "not json").unwrap();
        assert!(PluginConfig::load(&bad).is_err());

        let badrepo = dir.path().join("badrepo.json");
        std::fs::write(&badrepo, r#"{"repo":"noslash"}"#).unwrap();
        assert!(PluginConfig::load(&badrepo)
            .unwrap_err()
            .to_string()
            .contains("owner/name"));
    }
}
