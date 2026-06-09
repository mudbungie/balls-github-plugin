use crate::error::{PluginError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The only secret this plugin stores: a GitHub token, in `<auth-dir>/token.json`.
/// Core never reads this file; it only passes the directory path.
///
/// The token is BOUND to the `api_base` it was set up against (bl-2d6d). `repo`
/// config — including `api_base` — is data that can travel between checkouts, so
/// an `api_base` an attacker rewrote would otherwise silently send the `Bearer`
/// token to their host. Binding makes a changed `api_base` fail CLOSED (re-auth
/// required) instead of leaking the credential.
#[derive(Debug, Serialize, Deserialize)]
struct Token {
    api_base: String,
    token: String,
}

fn token_path(auth_dir: &Path) -> PathBuf {
    auth_dir.join("token.json")
}

/// Store `token`, bound to `api_base`, owner-only. The file is CREATED with mode
/// `0o600` (not chmod'd after a default-umask write), so it is never briefly
/// world-readable — closing the bl-2d6d TOCTOU window.
pub fn save_token(auth_dir: &Path, api_base: &str, token: &str) -> Result<()> {
    std::fs::create_dir_all(auth_dir)?;
    let path = token_path(auth_dir);
    let json = serde_json::to_string(&Token {
        api_base: api_base.to_string(),
        token: token.to_string(),
    })?;
    write_owner_only(&path, &json)?;
    restrict(&path)?; // enforce 0o600 even when overwriting a pre-existing file
    Ok(())
}

/// Load the token, but ONLY if it was set up for `api_base`. A mismatch (a config
/// whose `api_base` no longer matches the one the token was bound to) is refused
/// loudly rather than leaking the credential to the new host (bl-2d6d).
pub fn load_token(auth_dir: &Path, api_base: &str) -> Result<String> {
    let path = token_path(auth_dir);
    let data = std::fs::read_to_string(&path)
        .map_err(|e| PluginError::Auth(format!("no GitHub token ({}); run auth-setup", e)))?;
    let parsed: Token = serde_json::from_str(&data)
        .map_err(|e| PluginError::Auth(format!("corrupt token file: {}", e)))?;
    if parsed.api_base != api_base {
        return Err(PluginError::Auth(format!(
            "token was set up for {}, but config now targets {}; run auth-setup",
            parsed.api_base, api_base
        )));
    }
    Ok(parsed.token)
}

#[cfg(unix)]
fn write_owner_only(path: &Path, data: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, data: &str) -> Result<()> {
    std::fs::write(path, data)?;
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://api.github.com";

    #[test]
    fn save_then_load_roundtrip_for_the_same_api_base() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("github");
        save_token(&nested, BASE, "ghp_abc").unwrap();
        assert_eq!(load_token(&nested, BASE).unwrap(), "ghp_abc");
    }

    #[test]
    fn load_refuses_a_token_bound_to_a_different_api_base() {
        // The exfil guard: a token set up for github.com is NOT handed out when
        // the config now points somewhere else (a rewritten base.json).
        let dir = tempfile::tempdir().unwrap();
        save_token(dir.path(), BASE, "ghp_secret").unwrap();
        let err = load_token(dir.path(), "https://evil.example/api/v3")
            .unwrap_err()
            .to_string();
        assert!(err.contains("was set up for") && err.contains("run auth-setup"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        save_token(dir.path(), BASE, "t").unwrap();
        let mode = std::fs::metadata(token_path(dir.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn overwriting_a_loose_file_restricts_it() {
        // A pre-existing 0o644 file (legacy) is tightened to 0o600 on save.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = token_path(dir.path());
        std::fs::write(&path, "stale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        save_token(dir.path(), BASE, "t").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn load_missing_is_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_token(dir.path(), BASE)
            .unwrap_err()
            .to_string()
            .contains("auth-setup"));
    }

    #[test]
    fn load_corrupt_is_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(token_path(dir.path()), "not json").unwrap();
        assert!(load_token(dir.path(), BASE)
            .unwrap_err()
            .to_string()
            .contains("corrupt"));
    }
}
