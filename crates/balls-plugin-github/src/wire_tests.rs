use super::*;
use balls_github_shared::error::PluginError;

#[test]
fn parses_full_pre_wire() {
    let w = Wire::parse(
        r#"{"actor":"alice",
            "binding":{"invocation_path":"/proj","landing":"/land"},
            "current_state":{"title":"Do it","target_branch":"develop"}}"#,
    )
    .unwrap();
    assert_eq!(w.actor, "alice");
    assert_eq!(w.binding.invocation_path, "/proj");
    assert_eq!(w.binding.landing, "/land");
    let st = w.current_state.unwrap();
    assert_eq!(st.title, "Do it");
    assert_eq!(st.target_branch.as_deref(), Some("develop"));
    assert!(w.metadata.is_none());
    assert!(w.rolling_back.is_none());
}

#[test]
fn defaults_when_minimal() {
    let w = Wire::parse(r#"{"binding":{"invocation_path":"/p"}}"#).unwrap();
    assert_eq!(w.actor, "");
    assert_eq!(w.binding.landing, "");
    assert!(w.current_state.is_none());
}

#[test]
fn parses_rollback_and_metadata() {
    let w = Wire::parse(
        r#"{"binding":{"invocation_path":"/p"},
            "rolling_back":"post",
            "metadata":{"bl-id":["bl-1234"]}}"#,
    )
    .unwrap();
    assert_eq!(w.rolling_back.as_deref(), Some("post"));
    assert_eq!(w.metadata.unwrap()["bl-id"], vec!["bl-1234"]);
}

#[test]
fn bad_json_errors() {
    assert!(Wire::parse("not json").is_err());
}

#[test]
fn resolve_id_prefers_metadata() {
    let mut m = Metadata::new();
    m.insert("bl-id".into(), vec!["bl-aaaa".into()]);
    let id = resolve_id(Some(&m), || panic!("should not read changed paths")).unwrap();
    assert_eq!(id, "bl-aaaa");
}

#[test]
fn resolve_id_falls_back_to_single_changed_file() {
    let id = resolve_id(None, || Ok(vec!["tasks/bl-bbbb.md".into()])).unwrap();
    assert_eq!(id, "bl-bbbb");
}

#[test]
fn resolve_id_ignores_metadata_without_bl_id() {
    let m = Metadata::new();
    let id = resolve_id(Some(&m), || Ok(vec!["tasks/bl-cccc.md".into()])).unwrap();
    assert_eq!(id, "bl-cccc");
}

#[test]
fn resolve_id_rejects_zero_or_many() {
    let err = resolve_id(None, || Ok(vec![])).unwrap_err();
    assert!(matches!(err, PluginError::Other(_)));
    let err = resolve_id(None, || Ok(vec!["tasks/a.md".into(), "tasks/b.md".into()])).unwrap_err();
    assert!(err.to_string().contains("found 2"));
}

#[test]
fn resolve_id_propagates_changed_error() {
    let err = resolve_id(None, || Err(PluginError::Other("git boom".into()))).unwrap_err();
    assert!(err.to_string().contains("git boom"));
}
