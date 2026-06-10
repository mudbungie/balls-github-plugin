//! §7 plugin wire payload + the `bl list --json` slice the forge plugin decodes.
//!
//! balls-core only ever SERIALIZES the wire (`crate::wire` there); a plugin owns
//! the matching deserialize for exactly the fields it needs. There is **no
//! return channel** (§7): the forge plugin contributes by shelling `bl`, never
//! by printing values balls parses back. Its stdout is the §6 human product
//! (the minted gate child's id; sync's resolved-gate lines); core forwards it
//! verbatim and parses nothing.
//!
//! Both hooks this plugin handles are POST-side, so the op-start ball arrives
//! as `previous_state` (§7: the post wire has NO `current_state` — the landed
//! ball is derived from git, never the wire) and the id as the sealed `bl-id`
//! trailer in `metadata`.

use balls_github_shared::error::{PluginError, Result};
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
    /// The op-start ball on a post wire (`pre`'s `current_state` slid over, §7).
    #[serde(default)]
    pub previous_state: Option<State>,
    #[serde(default)]
    pub rolling_back: Option<String>,
}

/// The ball fields forge reads off the wire: the title (the gate child's
/// subject source) and the preserved extras (§3) — flattened unknown keys,
/// where the plugin's own join key lives on a gate child.
#[derive(Debug, Default, Deserialize)]
pub struct State {
    #[serde(default)]
    pub title: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl State {
    /// A preserved extra's string value, if the key is set.
    #[must_use]
    pub fn extra_str(&self, key: &str) -> Option<&str> {
        self.extra.get(key).and_then(serde_json::Value::as_str)
    }
}

impl Wire {
    /// Parse the §7 payload JSON from `s`.
    pub fn parse(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

/// The sealed id off a post wire — every hook this plugin handles is post-side,
/// so a missing `bl-id` trailer is a protocol error, never a fallback.
pub fn sealed_id(metadata: Option<&Metadata>) -> Result<String> {
    metadata_id(metadata)
        .map(str::to_string)
        .ok_or_else(|| PluginError::Other("post wire carried no bl-id trailer".into()))
}

/// A gate child the plugin minted: its id and the parent it gates. The join is
/// the plugin-namespaced preserved key (§3) the mint stamps on the gate child —
/// derived on every read, never stored plugin-side.
#[derive(Debug, PartialEq, Eq)]
pub struct Gate {
    pub id: String,
    pub parent: String,
}

/// Decode `bl list --json` (the bedrock projection: stored frontmatter + `id`)
/// into this plugin's open gate children — the rows carrying the preserved
/// `key`. Rows without the key (everyone else's tasks) are simply skipped.
pub fn open_gates(list_json: &str, key: &str) -> Result<Vec<Gate>> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(list_json)
        .map_err(|e| PluginError::Other(format!("bad `bl list --json` output: {e}")))?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let parent = row.get(key)?.as_str()?;
            let id = row.get("id")?.as_str()?;
            Some(Gate { id: id.to_string(), parent: parent.to_string() })
        })
        .collect())
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
