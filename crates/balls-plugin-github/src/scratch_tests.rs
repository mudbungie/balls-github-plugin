use super::*;

fn territory(tmp: &Path) -> Territory {
    Territory::new(tmp, "balls-plugin-github", "/home/me/proj")
}

#[test]
fn auth_dir_is_invocation_independent() {
    let dir = tempfile::tempdir().unwrap();
    let a = Territory::new(dir.path(), "p", "/proj/one").auth_dir();
    let b = Territory::new(dir.path(), "p", "/proj/two").auth_dir();
    assert_eq!(a, b);
    assert!(a.ends_with("balls/plugins/p/auth"));
}

#[test]
fn remember_recall_forget_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let t = territory(dir.path());
    assert_eq!(t.recall_gate("bl-p").unwrap(), None);

    t.remember_gate("bl-p", "bl-gate").unwrap();
    assert_eq!(t.recall_gate("bl-p").unwrap().as_deref(), Some("bl-gate"));

    t.forget_gate("bl-p").unwrap();
    assert_eq!(t.recall_gate("bl-p").unwrap(), None);
    // forget is idempotent
    t.forget_gate("bl-p").unwrap();
}

#[test]
fn pending_gates_lists_sorted_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let t = territory(dir.path());
    // no gates dir yet -> empty
    assert!(t.pending_gates().unwrap().is_empty());

    t.remember_gate("bl-b", "bl-gb").unwrap();
    t.remember_gate("bl-a", "bl-ga").unwrap();
    assert_eq!(
        t.pending_gates().unwrap(),
        vec![("bl-a".to_string(), "bl-ga".to_string()), ("bl-b".to_string(), "bl-gb".to_string())]
    );
}

/// The territory's gates dir for the fixed test invocation.
fn gates_dir(state: &Path) -> PathBuf {
    state.join("balls/plugins/balls-plugin-github/by-project/home/me/proj/gates")
}

#[test]
fn recall_and_forget_surface_non_notfound_io_errors() {
    let dir = tempfile::tempdir().unwrap();
    let t = territory(dir.path());
    // a DIRECTORY where a gate file is expected: read_to_string / remove_file
    // both fail with a non-NotFound error (not the clean "absent" path).
    let as_dir = gates_dir(dir.path()).join("bl-x");
    std::fs::create_dir_all(&as_dir).unwrap();
    assert!(t.recall_gate("bl-x").is_err());
    assert!(t.forget_gate("bl-x").is_err());
}

#[test]
fn pending_gates_surfaces_a_non_notfound_read_dir_error() {
    let dir = tempfile::tempdir().unwrap();
    let t = territory(dir.path());
    // make `gates` a FILE so read_dir fails with NotADirectory, not NotFound.
    let gates = gates_dir(dir.path());
    std::fs::create_dir_all(gates.parent().unwrap()).unwrap();
    std::fs::write(&gates, "not a dir").unwrap();
    assert!(t.pending_gates().is_err());
}

#[test]
fn invocation_path_is_mirrored_not_encoded() {
    let dir = tempfile::tempdir().unwrap();
    let t = territory(dir.path());
    t.remember_gate("bl-p", "bl-g").unwrap();
    let p = dir.path().join("balls/plugins/balls-plugin-github/by-project/home/me/proj/gates/bl-p");
    assert!(p.exists());
}
