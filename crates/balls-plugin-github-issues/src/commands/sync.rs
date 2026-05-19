//! Sync half (GH -> balls via SyncReport). B1 lands a silent noop —
//! emits `{}` (an empty SyncReport per balls's plugin protocol,
//! identical to "nothing changed"). B4a–d wire identity matching +
//! loop avoidance and the three entry kinds (created / updated /
//! deleted) once the corresponding behavior balls land.

use balls_github_shared::error::Result;
use std::path::Path;

pub fn run(_filter: Option<&str>, _config_path: &Path, _auth_dir: &Path) -> Result<()> {
    println!("{{}}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_returns_empty_sync_report() {
        let p = Path::new("/dev/null");
        run(None, p, p).unwrap();
        run(Some("bl-x"), p, p).unwrap();
    }
}
