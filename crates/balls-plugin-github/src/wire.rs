//! §7 plugin wire payload — the slice the forge plugin decodes off stdin.
//!
//! balls-core only ever SERIALIZES the wire (`crate::wire` there); a plugin owns
//! the matching deserialize for exactly the fields it needs. There is **no
//! return channel** (§7): the forge plugin contributes by shelling `bl` and
//! pushing the project repo, never by printing values balls parses back. Its
//! stdout is a human hint (the PR URL); core forwards it verbatim and parses
//! nothing.

use balls_github_shared::error::Result;
pub use balls_github_shared::wire::{metadata_id, Binding, Metadata};
use serde::Deserialize;

/// The §7 fields the forge plugin reads.
#[derive(Debug, Deserialize)]
pub struct Wire {
    /// The invoking identity (`--as`). Threaded into `bl create`/`bl close` for
    /// the gate child so its lifecycle is stamped with the same actor.
    #[serde(default)]
    pub actor: String,
    pub binding: Binding,
    #[serde(default)]
    pub metadata: Option<Metadata>,
    #[serde(default)]
    pub current_state: Option<State>,
    #[serde(default)]
    pub rolling_back: Option<String>,
}

/// The ball fields forge reads: the title (the gate-child + PR subject) and a
/// per-task `target_branch` override (a preserved extra key, §3) that wins over
/// the config default for the PR base.
#[derive(Debug, Default, Deserialize)]
pub struct State {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub target_branch: Option<String>,
}

impl Wire {
    /// Parse the §7 payload JSON from `s`.
    pub fn parse(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

/// Resolve the op's task id. A post hook (`claim`/`unclaim`) carries it as the
/// sealed `bl-id` trailer in `metadata`; a pre hook (`close`) does not — it is
/// read back from the single changed `tasks/<id>.md` the op staged (`changed`
/// lists those paths, run lazily so git is only spawned on the pre path). Zero
/// or many changed task files is a protocol error.
pub fn resolve_id(
    metadata: Option<&Metadata>,
    changed: impl FnOnce() -> Result<Vec<String>>,
) -> Result<String> {
    if let Some(id) = metadata_id(metadata) {
        return Ok(id.to_string());
    }
    let ids: Vec<String> = changed()?
        .iter()
        .filter_map(|p| p.strip_prefix("tasks/").and_then(|s| s.strip_suffix(".md")))
        .map(str::to_string)
        .collect();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        other => Err(balls_github_shared::error::PluginError::Other(format!(
            "expected exactly one changed task file, found {}",
            other.len()
        ))),
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
