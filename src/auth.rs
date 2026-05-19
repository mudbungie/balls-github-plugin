use crate::error::{PluginError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The only secret this plugin stores: a GitHub token, in `<auth-dir>/token.json`.
/// Core never reads this file; it only passes the directory path.
#[derive(Debug, Serialize, Deserialize)]
struct Token {
    token: String,
}

fn token_path(auth_dir: &Path) -> PathBuf {
    auth_dir.join("token.json")
}

pub fn save_token(auth_dir: &Path, token: &str) -> Result<()> {
    std::fs::create_dir_all(auth_dir)?;
    let path = token_path(auth_dir);
    std::fs::write(
        &path,
        serde_json::to_string(&Token {
            token: token.to_string(),
        })?,
    )?;
    restrict(&path)?;
    Ok(())
}

pub fn load_token(auth_dir: &Path) -> Result<String> {
    let path = token_path(auth_dir);
    let data = std::fs::read_to_string(&path)
        .map_err(|e| PluginError::Auth(format!("no GitHub token ({}); run auth-setup", e)))?;
    let parsed: Token = serde_json::from_str(&data)
        .map_err(|e| PluginError::Auth(format!("corrupt token file: {}", e)))?;
    Ok(parsed.token)
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

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("github");
        save_token(&nested, "ghp_abc").unwrap();
        assert_eq!(load_token(&nested).unwrap(), "ghp_abc");
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        save_token(dir.path(), "t").unwrap();
        let mode = std::fs::metadata(token_path(dir.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn load_missing_is_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_token(dir.path())
            .unwrap_err()
            .to_string()
            .contains("auth-setup"));
    }

    #[test]
    fn load_corrupt_is_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(token_path(dir.path()), "not json").unwrap();
        assert!(load_token(dir.path())
            .unwrap_err()
            .to_string()
            .contains("corrupt"));
    }
}
