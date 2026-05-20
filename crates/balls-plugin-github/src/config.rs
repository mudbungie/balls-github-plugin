use crate::error::{PluginError, Result};
use serde::Deserialize;
use std::path::Path;

/// Plugin config, git-tracked at `.balls/plugins/github.json`. Non-secret.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginConfig {
    /// `owner/name`, e.g. `mudbungie/balls`.
    pub repo: String,
    /// Default PR base. Per-task `target_branch` overrides it. Deferred-mode
    /// review requires a target somewhere; `None` here and on the task is an
    /// error at push time, never silently `main`.
    #[serde(default)]
    pub target_branch: Option<String>,
    /// API root. Override for GitHub Enterprise.
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_api_base() -> String {
    "https://api.github.com".to_string()
}

impl PluginConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| PluginError::Config(format!("{}: {}", path.display(), e)))?;
        let cfg: PluginConfig = serde_json::from_str(&data)
            .map_err(|e| PluginError::Config(format!("{}: {}", path.display(), e)))?;
        if cfg.owner_name().is_none() {
            return Err(PluginError::Config(format!(
                "repo must be \"owner/name\", got {:?}",
                cfg.repo
            )));
        }
        Ok(cfg)
    }

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
    fn owner_name_rejects_bad_forms() {
        let bad = |r: &str| {
            serde_json::from_str::<PluginConfig>(&format!(r#"{{"repo":{:?}}}"#, r))
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
    fn load_roundtrip_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.json");
        std::fs::write(&ok, r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(PluginConfig::load(&ok).unwrap().repo, "o/n");

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
