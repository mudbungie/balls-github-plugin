//! Shared §7 plugin-wire shapes both GitHub plugins decode off stdin.
//!
//! balls-core only ever SERIALIZES the wire; a plugin owns the matching
//! deserialize for exactly the fields it needs (there is **no return channel**,
//! §7). The §7 binding paths and the §5 trailer metadata are byte-identical
//! across both plugins, so the shape lives here once; each plugin keeps only its
//! own op-specific payload wrapper (`Wire`/`Payload`) around it.

use serde::Deserialize;
use std::collections::BTreeMap;

/// §5 trailer metadata: a key → its values. The sealed `bl-id` lives here on a
/// post wire (it is not on a pre wire — the id is not sealed yet, §7).
pub type Metadata = BTreeMap<String, Vec<String>>;

/// The §7 binding paths the GitHub plugins read. `invocation_path` — the project
/// root they push / shell `bl` in, and the cwd-key for plugin territory (§7/§11)
/// — is always present. `landing` (the `balls/config` checkout holding committed
/// plugin config, §1/§4) and `store` (the `tasks/` checkout the issues plugin
/// reads the ball from, §13) default empty, so the one type decodes a forge wire
/// (no `store`), an issues wire, and a diffless `sync` wire alike.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Binding {
    pub invocation_path: String,
    #[serde(default)]
    pub landing: String,
    #[serde(default)]
    pub store: String,
}

/// The sealed ball id from a post wire's §5 `bl-id` trailer — `None` on a pre
/// wire, where the id is not sealed yet (§7). The single place either plugin
/// reaches into `metadata` for the id.
#[must_use]
pub fn metadata_id(metadata: Option<&Metadata>) -> Option<&str> {
    metadata?.get("bl-id")?.first().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_id_reads_the_sealed_trailer() {
        let mut m = Metadata::new();
        m.insert("bl-id".into(), vec!["bl-1a2b".into()]);
        m.insert("bl-op".into(), vec!["close".into()]);
        assert_eq!(metadata_id(Some(&m)), Some("bl-1a2b"));
    }

    #[test]
    fn metadata_id_is_none_without_metadata_or_bl_id() {
        assert_eq!(metadata_id(None), None);
        let empty = Metadata::new();
        assert_eq!(metadata_id(Some(&empty)), None);
        let mut no_id = Metadata::new();
        no_id.insert("bl-id".into(), Vec::new()); // present key, empty values
        assert_eq!(metadata_id(Some(&no_id)), None);
    }

    #[test]
    fn binding_decodes_partial_and_full_shapes() {
        let forge: Binding =
            serde_json::from_str(r#"{"invocation_path":"/p","landing":"/l"}"#).unwrap();
        assert_eq!(forge.invocation_path, "/p");
        assert_eq!(forge.landing, "/l");
        assert_eq!(forge.store, ""); // defaulted — a forge wire carries no store

        let issues: Binding =
            serde_json::from_str(r#"{"invocation_path":"/p","store":"/s"}"#).unwrap();
        assert_eq!(issues.store, "/s");
        assert_eq!(issues.landing, "");
    }
}
