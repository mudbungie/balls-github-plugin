use super::*;
use crate::wire::Wire;

#[test]
fn api_base_defaults_or_takes_the_arg() {
    assert_eq!(api_base(&["auth-setup".to_string()]), "https://api.github.com");
    assert_eq!(api_base(&["auth-setup".to_string(), "https://ghe.x/api/v3".to_string()]), "https://ghe.x/api/v3");
}

#[test]
fn config_path_is_under_the_landing() {
    let p = config_path("/land", "balls-plugin-github");
    assert!(p.ends_with("config/plugins/balls-plugin-github.json"));
    assert!(p.starts_with("/land"));
}

#[test]
fn ctx_of_reads_id_title_and_join_key_off_the_post_wire() {
    let w = Wire::parse(
        r#"{"binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]},
            "previous_state":{"title":"T","k":"bl-elder"}}"#,
    )
    .unwrap();
    let c = ctx_of("claim", &w, "k").unwrap();
    assert_eq!((c.id.as_str(), c.title.as_str(), c.gate_of.as_deref()), ("bl-1", "T", Some("bl-elder")));
}

#[test]
fn ctx_of_sync_is_empty_and_needs_no_metadata() {
    let w = Wire::parse(r#"{"binding":{"invocation_path":"/p"}}"#).unwrap();
    let c = ctx_of("sync", &w, "k").unwrap();
    assert!(c.id.is_empty() && c.gate_of.is_none());
}

#[test]
fn ctx_of_tolerates_a_missing_previous_state_but_not_a_missing_id() {
    let w = Wire::parse(r#"{"binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]}}"#).unwrap();
    let c = ctx_of("claim", &w, "k").unwrap();
    assert_eq!((c.id.as_str(), c.title.as_str()), ("bl-1", ""));

    let no_id = Wire::parse(r#"{"binding":{"invocation_path":"/p"}}"#).unwrap();
    assert!(ctx_of("claim", &no_id, "k").is_err());
}

#[test]
fn emit_writes_a_line_only_when_present() {
    let mut buf = Vec::new();
    emit(Some("bl-gate".into()), &mut buf).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "bl-gate\n");

    let mut empty = Vec::new();
    emit(None, &mut empty).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn usage_mentions_the_forms() {
    assert!(usage().to_string().contains("protocol"));
}
