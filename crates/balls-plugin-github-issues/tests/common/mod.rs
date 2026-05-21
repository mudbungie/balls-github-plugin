//! Shared helpers for the integration test suite. Cargo treats each
//! .rs file under `tests/` as a separate test crate, but a
//! `tests/common/` subdirectory is shared module code.
//!
//! `#[allow(dead_code)]` because each test crate only uses the
//! subset of helpers it needs; the unused ones would otherwise
//! flag dead_code on per-crate compile.

#![allow(dead_code)]

use assert_cmd::Command;
use std::path::{Path, PathBuf};

pub fn write_config(dir: &Path, api_base: &str) -> PathBuf {
    let p = dir.join("github-issues.json");
    std::fs::write(&p, format!(r#"{{"repo":"o/n","api_base":"{}"}}"#, api_base)).unwrap();
    p
}

pub fn write_config_with_label(dir: &Path, api_base: &str, label: &str) -> PathBuf {
    let p = dir.join("github-issues.json");
    std::fs::write(
        &p,
        format!(
            r#"{{"repo":"o/n","api_base":"{}","target_label":"{}"}}"#,
            api_base, label
        ),
    )
    .unwrap();
    p
}

pub fn write_token(dir: &Path) {
    std::fs::write(dir.join("token.json"), r#"{"token":"t"}"#).unwrap();
}

pub fn bin() -> Command {
    Command::cargo_bin("balls-plugin-github-issues").unwrap()
}

/// FNV-1a-64 hex digest. Mirrors `pull_content::body_hash` exactly.
/// Duplicated here (rather than imported) because integration tests
/// only see the binary's public surface; the coupling is intentional
/// — if the projection's hash algorithm ever changes, this helper
/// breaks too, which is the right place to find out.
pub fn fnv_hex(s: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h: u64 = FNV_OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:016x}")
}
