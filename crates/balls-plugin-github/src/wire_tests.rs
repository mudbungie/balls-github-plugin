use super::*;

const KEY: &str = "balls-plugin-github";

#[test]
fn parses_a_full_post_wire() {
    let w = Wire::parse(
        r#"{
            "protocol":1,"op":"claim","phase":"post","actor":"alice",
            "binding":{"invocation_path":"/proj","landing":"/land"},
            "metadata":{"bl-id":["bl-1"],"bl-op":["claim"]},
            "previous_state":{"title":"Do it","balls-plugin-github":"bl-elder"},
            "commit":"c2","previous_commit":"c1"
        }"#,
    )
    .unwrap();
    assert_eq!(w.actor, "alice");
    assert_eq!(w.binding.invocation_path, "/proj");
    assert_eq!(sealed_id(w.metadata.as_ref()).unwrap(), "bl-1");
    let s = w.previous_state.unwrap();
    assert_eq!(s.title, "Do it");
    assert_eq!(s.extra_str(KEY), Some("bl-elder"));
    assert_eq!(s.extra_str("absent"), None);
    assert!(w.rolling_back.is_none());
}

#[test]
fn parses_a_minimal_diffless_sync_wire() {
    // §13: sync carries no metadata, no states, no rolling_back.
    let w = Wire::parse(r#"{"binding":{"invocation_path":"/p"}}"#).unwrap();
    assert_eq!(w.actor, "");
    assert!(w.metadata.is_none());
    assert!(w.previous_state.is_none());
}

#[test]
fn extra_str_ignores_non_string_values() {
    let s: State = serde_json::from_str(r#"{"title":"t","balls-plugin-github":7}"#).unwrap();
    assert_eq!(s.extra_str(KEY), None);
}

#[test]
fn rollback_wire_carries_the_phase() {
    let w = Wire::parse(
        r#"{"binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]},"rolling_back":"post"}"#,
    )
    .unwrap();
    assert_eq!(w.rolling_back.as_deref(), Some("post"));
}

#[test]
fn sealed_id_errors_without_the_trailer() {
    let err = sealed_id(None).unwrap_err().to_string();
    assert!(err.contains("bl-id"), "{err}");
}

#[test]
fn open_gates_scans_rows_carrying_the_key() {
    let json = r#"[
        {"id":"bl-g1","title":"Review gate: x","balls-plugin-github":"bl-p"},
        {"id":"bl-w1","title":"ordinary work"},
        {"id":"bl-g2","title":"Review gate: y","balls-plugin-github":"bl-q"},
        {"id":"bl-odd","balls-plugin-github":3},
        {"title":"no id somehow","balls-plugin-github":"bl-r"}
    ]"#;
    let gates = open_gates(json, KEY).unwrap();
    assert_eq!(
        gates,
        vec![
            Gate { id: "bl-g1".into(), parent: "bl-p".into() },
            Gate { id: "bl-g2".into(), parent: "bl-q".into() },
        ]
    );
}

#[test]
fn open_gates_rejects_bad_json() {
    let err = open_gates("not json", KEY).unwrap_err().to_string();
    assert!(err.contains("bl list --json"), "{err}");
}
