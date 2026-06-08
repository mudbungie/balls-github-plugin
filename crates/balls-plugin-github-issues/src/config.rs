//! Issues plugin config — committed, non-secret, on the landing (§4) at
//! `<landing>/config/plugins/github-issues/config.json` (bl-613d).
//!
//! Three plugin-specific knobs sit on the shared [`RepoConfig`] (`repo`,
//! `api_base`). There is no longer a projection-prefix const: under the
//! no-return-channel protocol the plugin keeps no `external.github-issues.*`
//! blob on the ball — the join is the title marker (`crate::marker`) and the
//! reconciliation state is in territory (`crate::base`).

use balls_github_shared::config_base::{load_json, RepoConfig};
use balls_github_shared::error::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// What to do when a GH issue closes externally and we mirror that inward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseMirror {
    /// GH-side close flips the balls task closed (the strict default).
    #[default]
    Authoritative,
    /// Same inward mirror, but downstream treats failure as best-effort.
    BestEffort,
    /// GH never owns status; a GH close never touches balls.
    Off,
}

/// What to do when a previously-mirrored GH issue vanishes from the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnExternalDelete {
    /// Tag the task `deferred` — operator decides to revive or close.
    #[default]
    Deferred,
    /// Close the task.
    Closed,
    /// Do nothing.
    Noop,
}

/// The full issues-plugin config: shared repo half + the three knobs.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginConfig {
    #[serde(flatten)]
    pub base: RepoConfig,
    /// If set, only issues carrying this label are in sync scope.
    #[serde(default)]
    pub target_label: Option<String>,
    #[serde(default)]
    pub on_external_delete: OnExternalDelete,
    #[serde(default)]
    pub close_mirror: CloseMirror,
}

/// `<landing>/config/plugins/github-issues/config.json` — the committed config
/// path, derived from the §7 `binding.landing`.
#[must_use]
pub fn config_path(landing: &str) -> PathBuf {
    Path::new(landing)
        .join("config")
        .join("plugins")
        .join("github-issues")
        .join("config.json")
}

impl PluginConfig {
    /// Load + validate from an explicit path (the binary resolves it from the
    /// landing via [`config_path`]).
    pub fn load(path: &Path) -> Result<Self> {
        let cfg: PluginConfig = load_json(path)?;
        cfg.base.validate()?;
        Ok(cfg)
    }

    #[must_use]
    pub fn api_base(&self) -> &str {
        self.base.api_base()
    }

    /// `(owner, name)` — `repo` is validated on load, so this is `Some`.
    #[must_use]
    pub fn owner_name(&self) -> Option<(&str, &str)> {
        self.base.owner_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_uses_documented_defaults() {
        let cfg: PluginConfig = serde_json::from_str(r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(cfg.base.repo, "o/n");
        assert_eq!(cfg.api_base(), "https://api.github.com");
        assert!(cfg.target_label.is_none());
        assert_eq!(cfg.on_external_delete, OnExternalDelete::Deferred);
        assert_eq!(cfg.close_mirror, CloseMirror::Authoritative);
    }

    #[test]
    fn deserialize_full_overrides_each_default() {
        let cfg: PluginConfig = serde_json::from_str(
            r#"{"repo":"a/b","api_base":"https://ghe.x/api/v3",
                "target_label":"balls:track","on_external_delete":"closed","close_mirror":"off"}"#,
        )
        .unwrap();
        assert_eq!(cfg.api_base(), "https://ghe.x/api/v3");
        assert_eq!(cfg.target_label.as_deref(), Some("balls:track"));
        assert_eq!(cfg.on_external_delete, OnExternalDelete::Closed);
        assert_eq!(cfg.close_mirror, CloseMirror::Off);
        assert_eq!(cfg.owner_name(), Some(("a", "b")));
    }

    #[test]
    fn each_enum_variant_round_trips() {
        for (tag, want) in [
            ("authoritative", CloseMirror::Authoritative),
            ("best_effort", CloseMirror::BestEffort),
            ("off", CloseMirror::Off),
        ] {
            let cfg: PluginConfig =
                serde_json::from_str(&format!(r#"{{"repo":"o/n","close_mirror":"{tag}"}}"#)).unwrap();
            assert_eq!(cfg.close_mirror, want, "tag {tag}");
        }
        for (tag, want) in [
            ("deferred", OnExternalDelete::Deferred),
            ("closed", OnExternalDelete::Closed),
            ("noop", OnExternalDelete::Noop),
        ] {
            let cfg: PluginConfig =
                serde_json::from_str(&format!(r#"{{"repo":"o/n","on_external_delete":"{tag}"}}"#))
                    .unwrap();
            assert_eq!(cfg.on_external_delete, want, "tag {tag}");
        }
    }

    #[test]
    fn load_roundtrip_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.json");
        std::fs::write(&ok, r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(PluginConfig::load(&ok).unwrap().base.repo, "o/n");

        let badrepo = dir.path().join("badrepo.json");
        std::fs::write(&badrepo, r#"{"repo":"noslash"}"#).unwrap();
        assert!(PluginConfig::load(&badrepo).unwrap_err().to_string().contains("owner/name"));
    }

    #[test]
    fn config_path_is_under_the_landing() {
        assert_eq!(
            config_path("/clone/config"),
            PathBuf::from("/clone/config/config/plugins/github-issues/config.json"),
        );
    }
}
