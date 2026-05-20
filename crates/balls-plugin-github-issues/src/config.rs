//! Issues plugin config, git-tracked at
//! `.balls/plugins/github-issues.json`.
//!
//! Three plugin-specific knobs sit on top of the shared
//! `RepoConfig`. Each is per-instance, not per-event — per-event
//! failure policy lives in balls core's `.balls/config.json` per
//! SPEC-lifecycle-sync-participants §11. The defaults here encode
//! the policy Epic B locked in: GH-side close is authoritative;
//! external delete maps to deferred (operator decides what to do);
//! no label filter (every issue in the repo is in scope).
//!
//! The projection prefix is exported as a const so the
//! disjointness check against the forge plugin's projection lives
//! in code, not in convention.

use balls_github_shared::config_base::{load_json, RepoConfig};
use balls_github_shared::error::Result;
use serde::Deserialize;
use std::path::Path;

/// The `external.<name>.*` prefix this plugin owns authoritatively.
/// Forge plugin owns `external.github.*`; the prefix below is what
/// keeps them disjoint by construction. Don't change without
/// re-reading SPEC-lifecycle-sync-participants §3.
///
/// The participant-name segment must equal the plugin name as
/// configured in `.balls/config.json` (`github-issues`) — that
/// hyphenated form is the outer key core writes into
/// `task.external` per `PushResponse` semantics. `IssuesTaskExt`
/// derives the lookup key by stripping this prefix's `external.`
/// and trailing `.`, so a mismatch here makes every re-poll
/// classify as AutoCreate (the bl-a2ea regression).
#[allow(dead_code)]
pub const PROJECTION_PREFIX: &str = "external.github-issues.";

/// The forge plugin's projection prefix — kept here as a hardcoded
/// string for the disjointness test. If the forge plugin ever
/// renames its projection, this constant must update.
#[allow(dead_code)]
pub const FORGE_PROJECTION_PREFIX: &str = "external.github.";

/// What to do when a GH issue closes externally and we mirror that
/// back to the balls task. `Authoritative` is the documented default
/// (Epic B's decision); operators can dial it back via config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseMirror {
    /// GH-issue close → balls task `status=closed` via SyncReport.
    #[default]
    Authoritative,
    /// Same emission, but downstream failure-policy treats it as
    /// best-effort (warns + records sync_status; does not abort the
    /// event). This is a *policy* opt-down — the plugin still emits.
    BestEffort,
    /// Do not mirror close at all. The GH-side close is recorded in
    /// `external.github-issues.state` but balls's `status` is
    /// untouched. Symmetric "GH never owns status".
    Off,
}

/// What to do when a previously-mirrored GH issue vanishes (404 on
/// fetch by stored number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnExternalDelete {
    /// Flip the balls task to `status=deferred` with a note. The
    /// operator decides whether to revive or close. Default per
    /// SKILL.md ("set aside with intent to revisit").
    #[default]
    Deferred,
    /// Flip the balls task to `status=closed`. The work is treated
    /// as archived; recoverable from the state branch.
    Closed,
    /// Leave the balls task alone. The orphan mapping persists in
    /// `external.github-issues` until manually cleared.
    Noop,
}

// B2 declares these knobs as part of the config contract; the
// runtime consumers land in B3/B4/B5. `dead_code` until then is
// honest documentation, not a bug.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct PluginConfig {
    #[serde(flatten)]
    pub base: RepoConfig,
    /// If set, sync only mirrors GH issues carrying this label.
    /// Unset = every issue in the repo is in scope (Epic B default).
    /// Consumed by B4a's classifier.
    #[serde(default)]
    pub target_label: Option<String>,
    /// See [`OnExternalDelete`]. Defaults to `Deferred`. Consumed
    /// by B4d's policy switch.
    #[serde(default)]
    pub on_external_delete: OnExternalDelete,
    /// See [`CloseMirror`]. Defaults to `Authoritative` (Epic B's
    /// locked-in policy). Consumed by B4b's emit decision.
    #[serde(default)]
    pub close_mirror: CloseMirror,
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
    fn deserialize_minimal_uses_documented_defaults() {
        let cfg: PluginConfig = serde_json::from_str(r#"{"repo":"o/n"}"#).unwrap();
        assert_eq!(cfg.repo(), "o/n");
        assert_eq!(cfg.api_base(), "https://api.github.com");
        assert!(cfg.target_label.is_none());
        assert_eq!(cfg.on_external_delete, OnExternalDelete::Deferred);
        assert_eq!(cfg.close_mirror, CloseMirror::Authoritative);
    }

    #[test]
    fn deserialize_full_overrides_each_default() {
        let cfg: PluginConfig = serde_json::from_str(
            r#"{
                "repo":"a/b",
                "api_base":"https://ghe.x/api/v3",
                "target_label":"balls:track",
                "on_external_delete":"closed",
                "close_mirror":"off"
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.api_base(), "https://ghe.x/api/v3");
        assert_eq!(cfg.target_label.as_deref(), Some("balls:track"));
        assert_eq!(cfg.on_external_delete, OnExternalDelete::Closed);
        assert_eq!(cfg.close_mirror, CloseMirror::Off);
    }

    #[test]
    fn each_enum_variant_round_trips() {
        for (tag, want) in [
            ("authoritative", CloseMirror::Authoritative),
            ("best_effort", CloseMirror::BestEffort),
            ("off", CloseMirror::Off),
        ] {
            let cfg: PluginConfig = serde_json::from_str(&format!(
                r#"{{"repo":"o/n","close_mirror":"{tag}"}}"#
            ))
            .unwrap();
            assert_eq!(cfg.close_mirror, want, "tag {tag}");
        }
        for (tag, want) in [
            ("deferred", OnExternalDelete::Deferred),
            ("closed", OnExternalDelete::Closed),
            ("noop", OnExternalDelete::Noop),
        ] {
            let cfg: PluginConfig = serde_json::from_str(&format!(
                r#"{{"repo":"o/n","on_external_delete":"{tag}"}}"#
            ))
            .unwrap();
            assert_eq!(cfg.on_external_delete, want, "tag {tag}");
        }
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

    #[test]
    fn projection_prefix_is_disjoint_from_forge() {
        // Neither prefix is a substring of the other: a key under
        // `external.github-issues.foo` is NOT under
        // `external.github.foo` and vice versa. This is the
        // disjointness contract from
        // SPEC-lifecycle-sync-participants §3.
        assert_ne!(PROJECTION_PREFIX, FORGE_PROJECTION_PREFIX);
        assert!(!PROJECTION_PREFIX.starts_with(FORGE_PROJECTION_PREFIX));
        assert!(!FORGE_PROJECTION_PREFIX.starts_with(PROJECTION_PREFIX));
    }
}
