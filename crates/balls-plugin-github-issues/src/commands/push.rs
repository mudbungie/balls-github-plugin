//! Push half (balls -> GH issue). B1 lands a silent noop — the
//! binary registers as a participant and is invoked on subscribed
//! events, but emits an empty response so the protocol contract is
//! satisfied without doing anything yet. B3 wires the real
//! create/update/close mirror logic.

use balls_github_shared::error::Result;
use std::path::Path;

pub fn run(_task_id: &str, _config_path: &Path, _auth_dir: &Path) -> Result<()> {
    println!("{{}}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_returns_empty_json() {
        // The unit-level test is trivially-narrow: run() returns Ok
        // and writes only to stdout (captured by the integration
        // tests in tests/cli.rs, which assert the literal `{}` body).
        let p = Path::new("/dev/null");
        run("bl-x", p, p).unwrap();
    }
}
