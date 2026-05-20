//! Compile-time-ish guard for `lib.rs`'s boundary invariant: the
//! shared crate must not reference per-plugin projection namespaces.
//!
//! Each plugin owns its own `external.<name>.*` projection in its own
//! crate. If a per-plugin literal appears in the shared sources, a
//! *plugin-specific* concern has leaked into shared code — fix the
//! design, don't loosen this test.

use std::fs;
use std::path::Path;

fn scan_dir(dir: &Path, forbidden: &[&str], hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_dir(&p, forbidden, hits);
            continue;
        }
        if p.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        // Skip this file — it spells out the forbidden tokens as data,
        // which would self-trigger the check otherwise.
        if p.file_name() == Some(std::ffi::OsStr::new("projection_boundary_test.rs")) {
            continue;
        }
        let body = fs::read_to_string(&p).unwrap();
        for tok in forbidden {
            if body.contains(tok) {
                hits.push(format!("{}: mentions {tok}", p.display()));
            }
        }
    }
}

#[test]
fn shared_has_no_per_plugin_projection_refs() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = Path::new(&manifest).join("src");
    let forbidden = &["external.github.", "external.github-issues."];
    let mut hits = Vec::new();
    scan_dir(&src, forbidden, &mut hits);
    assert!(
        hits.is_empty(),
        "shared crate references plugin projections:\n  {}",
        hits.join("\n  ")
    );
}

#[test]
fn scan_recurses_subdirs_and_skips_non_rust_files() {
    // Build a small fixture tree with one nested .rs file carrying a
    // forbidden token, one non-.rs file with the token (must be
    // ignored), and one clean .rs file. Confirms every branch of
    // scan_dir is exercised: directory recursion, non-rs skip, the
    // self-skip filename, the hit path, and the clean path.
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(
        sub.join("dirty.rs"),
        "fn x() { let _ = \"forbidden-token-A\"; }",
    )
    .unwrap();
    fs::write(sub.join("notes.txt"), "forbidden-token-A in non-rs").unwrap();
    fs::write(dir.path().join("clean.rs"), "fn ok() {}").unwrap();
    fs::write(
        dir.path().join("projection_boundary_test.rs"),
        "this file is always skipped: forbidden-token-A",
    )
    .unwrap();

    let mut hits = Vec::new();
    scan_dir(dir.path(), &["forbidden-token-A"], &mut hits);

    // Exactly the nested .rs file should hit. The non-rs file, the
    // clean file, and the self-named file are all silent.
    assert_eq!(hits.len(), 1, "unexpected hits: {hits:?}");
    assert!(hits[0].contains("dirty.rs"));
    assert!(hits[0].contains("forbidden-token-A"));
}
