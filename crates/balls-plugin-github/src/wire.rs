//! §7 plugin wire payload — the slice the forge plugin decodes off stdin.
//!
//! balls-core only ever SERIALIZES the wire (`crate::wire` there); a plugin owns
//! the matching deserialize for exactly the fields it needs. There is **no
//! return channel** (§7): the forge plugin contributes by shelling `bl` and
//! pushing the project repo, never by printing values balls parses back. Its
//! stdout is a human hint (the PR URL); core forwards it verbatim and parses
//! nothing.

use balls_github_shared::error::Result;
use serde::Deserialize;
use std::collections::BTreeMap;

/// §5 trailer metadata: a key → its values. The sealed `bl-id` lives here on a
/// post wire (it is not on a pre wire — the id is not sealed yet, §7).
pub type Metadata = BTreeMap<String, Vec<String>>;

/// The §7 fields the forge plugin reads.
#[derive(Debug, Deserialize)]
pub struct Wire {
    /// The invoking identity (`--as`). Threaded into `bl create/close/drop` for
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

/// The binding fields forge needs: the project-repo root it pushes (§7/§11) and
/// the landing checkout where its committed config lives (§4/§6). `landing`
/// defaults empty — a diffless `sync` wire still carries it, but a fixture need
/// not.
#[derive(Debug, Default, Deserialize)]
pub struct Binding {
    pub invocation_path: String,
    #[serde(default)]
    pub landing: String,
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

/// Resolve the op's task id. A post hook (`claim`/`drop`) carries it as the
/// sealed `bl-id` trailer in `metadata`; a pre hook (`close`) does not — it is
/// read back from the single changed `tasks/<id>.md` the op staged (`changed`
/// lists those paths, run lazily so git is only spawned on the pre path). Zero
/// or many changed task files is a protocol error.
pub fn resolve_id(
    metadata: Option<&Metadata>,
    changed: impl FnOnce() -> Result<Vec<String>>,
) -> Result<String> {
    if let Some(id) = metadata.and_then(|m| m.get("bl-id")).and_then(|v| v.first()) {
        return Ok(id.clone());
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
