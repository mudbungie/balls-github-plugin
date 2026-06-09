use super::*;

fn env(state: &std::path::Path) -> Env {
    Env {
        plugin_name: "balls-plugin-github".into(),
        state_home: state.to_path_buf(),
        bl_program: "bl".into(),
        cwd: state.to_path_buf(),
    }
}

#[test]
fn setup_validates_then_stores_the_token() {
    let mut s = mockito::Server::new();
    s.mock("GET", "/user").with_status(200).with_body(r#"{"login":"octocat"}"#).create();
    let dir = tempfile::tempdir().unwrap();
    let e = env(dir.path());

    let mut out = Vec::new();
    setup(&e, &s.url(), "ghp_tok", &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("octocat"));
    // token landed in the territory auth dir
    assert_eq!(auth::load_token(&auth_dir(&e), &s.url()).unwrap(), "ghp_tok");
}

#[test]
fn setup_rejects_a_bad_token_without_storing() {
    let mut s = mockito::Server::new();
    s.mock("GET", "/user").with_status(401).with_body("nope").create();
    let dir = tempfile::tempdir().unwrap();
    let e = env(dir.path());

    let mut out = Vec::new();
    assert!(setup(&e, &s.url(), "bad", &mut out).is_err());
    assert!(auth::load_token(&auth_dir(&e), &s.url()).is_err()); // nothing written
}

#[test]
fn check_verifies_a_stored_token() {
    let mut s = mockito::Server::new();
    s.mock("GET", "/user").with_status(200).with_body(r#"{"login":"me"}"#).create();
    let dir = tempfile::tempdir().unwrap();
    let e = env(dir.path());
    auth::save_token(&auth_dir(&e), &s.url(), "t").unwrap();

    let mut out = Vec::new();
    check(&e, &s.url(), &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("me"));
}

#[test]
fn check_errors_without_a_stored_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    assert!(check(&env(dir.path()), "http://x", &mut out).is_err());
}
