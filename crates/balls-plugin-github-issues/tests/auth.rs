//! Integration tests for `auth-setup` and `auth-check`.

mod common;
use common::{bin, write_config, write_token};

#[test]
fn auth_setup_reads_token_from_stdin() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/user")
        .with_status(200)
        .with_body(r#"{"login":"octocat"}"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());

    bin()
        .args(["auth-setup", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .write_stdin("ghp_fromstdin\n")
        .assert()
        .success();

    let stored = std::fs::read_to_string(dir.path().join("token.json")).unwrap();
    assert!(stored.contains("ghp_fromstdin"));
}

#[test]
fn auth_check_succeeds_with_valid_token() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/user")
        .with_status(200)
        .with_body(r#"{"login":"x"}"#)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &server.url());
    write_token(dir.path());

    bin()
        .args(["auth-check", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn missing_token_errors_and_exits_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), "https://api.github.com");

    bin()
        .args(["auth-check", "--config"])
        .arg(&cfg)
        .arg("--auth-dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("error:"));
}
