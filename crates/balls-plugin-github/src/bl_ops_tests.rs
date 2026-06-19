use super::*;
use std::os::unix::fs::PermissionsExt;

/// Write an executable fake `bl` whose body is `body`, and return its path.
fn fake_bl(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("bl");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

#[test]
fn create_gate_parses_id_and_mints_an_explicit_close_gate() {
    let dir = tempfile::tempdir().unwrap();
    // print the id on stdout, record argv to args.txt in cwd
    let bl = fake_bl(dir.path(), r#"echo bl-newid; printf '%s\n' "$*" > args.txt"#);
    let runner = Bl::new(&bl, dir.path(), "alice");

    assert_eq!(runner.create_gate("bl-p", "Do it").unwrap(), "bl-newid");
    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    // The gate is an EXPLICIT close-gate edge: `--parent X --blocks close`, not
    // `--subtask-of` (which gates X's CLAIM since bl-5d9a; bl-788e sugar dropped).
    assert!(args.contains("create --parent bl-p --blocks close --as alice"), "args: {args}");
    // The task-sourced title rides behind `--` (end-of-options): a hostile
    // `-`-leading title can never hijack a flag.
    assert!(args.contains("-- Review gate: Do it"), "args: {args}");
}

#[test]
fn create_gate_tolerates_a_leading_log_line() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), r#"echo '{"lvl":"info"}'; echo bl-xyz1"#);
    let runner = Bl::new(&bl, dir.path(), "a");
    assert_eq!(runner.create_gate("bl-p", "t").unwrap(), "bl-xyz1");
}

#[test]
fn create_gate_errors_when_no_id_is_minted() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), "echo nothing-here");
    let runner = Bl::new(&bl, dir.path(), "a");
    assert!(runner.create_gate("bl-p", "t").unwrap_err().to_string().contains("minted no id"));
}

#[test]
fn set_extra_stamps_the_join_key() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), r#"printf '%s\n' "$*" > last.txt"#);
    let runner = Bl::new(&bl, dir.path(), "alice");

    runner.set_extra("bl-g", "balls-plugin-github", "bl-p").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("last.txt")).unwrap().trim(),
        "update bl-g balls-plugin-github=bl-p --as alice"
    );
}

#[test]
fn close_runs_the_verb_with_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), r#"printf '%s\n' "$*" > last.txt"#);
    let runner = Bl::new(&bl, dir.path(), "bob");

    runner.close("bl-g", "PR merged: u").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("last.txt")).unwrap().trim(),
        "close bl-g -m PR merged: u --as bob"
    );
}

#[test]
fn list_json_returns_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), r"echo '[]'");
    let runner = Bl::new(&bl, dir.path(), "a");
    assert_eq!(runner.list_json().unwrap().trim(), "[]");
}

#[test]
fn a_busy_executable_is_retried_then_surfaced() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), "exit 0");
    // hold the script open for write the whole time → exec keeps getting ETXTBSY,
    // so the bounded retry exhausts and the error surfaces (rather than hanging).
    let _hold = std::fs::OpenOptions::new().write(true).open(&bl).unwrap();
    let runner = Bl::new(&bl, dir.path(), "a");
    assert!(runner.close("bl-g", "n").is_err());
}

#[test]
fn nonzero_exit_becomes_an_error_with_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let bl = fake_bl(dir.path(), "echo boom >&2; exit 1");
    let runner = Bl::new(&bl, dir.path(), "a");
    let err = runner.close("bl-g", "n").unwrap_err().to_string();
    assert!(err.contains("boom"), "{err}");
}
