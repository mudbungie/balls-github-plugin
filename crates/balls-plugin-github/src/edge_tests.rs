use super::*;

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
fn resolve_base_prefers_task_then_config() {
    assert_eq!(resolve_base(Some("dev".into()), Some("main".into()), "close").unwrap(), "dev");
    assert_eq!(resolve_base(None, Some("main".into()), "close").unwrap(), "main");
}

#[test]
fn resolve_base_requires_a_base_only_for_close() {
    assert!(resolve_base(None, None, "close").unwrap_err().to_string().contains("no target_branch"));
    // a non-close op tolerates an absent base (it never reads it)
    assert_eq!(resolve_base(None, None, "claim").unwrap(), "");
}

#[test]
fn emit_writes_a_line_only_when_present() {
    let mut buf = Vec::new();
    emit(Some("https://pr/1".into()), &mut buf).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "https://pr/1\n");

    let mut empty = Vec::new();
    emit(None, &mut empty).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn usage_mentions_the_forms() {
    assert!(usage().to_string().contains("protocol"));
}
