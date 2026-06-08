//! Integration coverage for the binary edge (`main.rs`): the §6 `protocol`
//! self-describe and the usage-error exit path, run through the built binary.

use assert_cmd::Command;
use predicates::str::contains;

fn bin() -> Command {
    Command::cargo_bin("balls-plugin-github-issues").unwrap()
}

#[test]
fn protocol_self_describes_over_the_process_boundary() {
    bin()
        .arg("protocol")
        .assert()
        .success()
        .stdout(contains(r#""protocol":1"#))
        .stdout(contains(r#""sync""#));
}

#[test]
fn an_unrecognized_invocation_exits_nonzero() {
    bin().args(["too", "many", "args"]).assert().failure();
}

#[test]
fn a_hook_with_a_malformed_payload_exits_nonzero() {
    // `sync post` reads a §7 payload from stdin; garbage aborts the op (§6).
    bin().args(["sync", "post"]).write_stdin("not json").assert().failure();
}
