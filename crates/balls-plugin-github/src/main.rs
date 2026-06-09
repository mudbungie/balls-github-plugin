//! `balls-plugin-github` — the §11 forge delivery variant, a §6/§7 subprocess
//! plugin.
//!
//! It is wired ALONGSIDE the worktree-owning `bl-delivery`, taking over only the
//! delivery hooks that make delivery go through a pull request instead of a local
//! squash (see [`forge`] for the matrix):
//!
//! ```text
//! claim.post = ["bl-delivery", "balls-plugin-github", "tracker"]  # worktree, then gate child
//! close.pre  = ["balls-plugin-github"]                            # push + PR, NOT a local squash
//! close.post = ["bl-delivery", "tracker"]                         # bl-delivery still tears the worktree down
//! sync.post  = ["balls-plugin-github"]                            # close the gate child on merge
//! drop.post  = ["bl-delivery", "balls-plugin-github", "tracker"]  # worktree + PR teardown
//! ```
//!
//! `main` only adapts the process boundary: it reads the §6 env ONCE here (the
//! bl-bfa8 rule — no env reads in the library modules), slurps stdin (the §7
//! wire, or the token for `auth-setup`), and hands off to [`edge::handle`].

mod authcmd;
mod bl_ops;
mod config;
mod edge;
mod forge;
mod git_ops;
mod pr_api;
mod project;
mod scratch;
mod wire;

use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::exit;

/// The plugin's own name — the User-Agent it sends, and the territory/config
/// default when balls does not name it via `BALLS_PLUGIN_NAME`.
pub const USER_AGENT: &str = "balls-plugin-github";

/// The process-edge environment the §6 dispatch needs, read ONCE in [`main`].
pub struct Env {
    pub plugin_name: String,
    pub state_home: PathBuf,
    pub bl_program: PathBuf,
    pub cwd: PathBuf,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let env = read_env();
    // `protocol`/hooks read the §7 wire here; `auth-setup` reads the token here.
    // A closed/empty stdin is simply an empty string.
    let mut stdin = String::new();
    let _ = io::stdin().read_to_string(&mut stdin);
    let mut out = io::stdout().lock();
    if let Err(e) = edge::handle(&args, &env, &stdin, &mut out) {
        let _ = out.flush();
        eprintln!("{USER_AGENT}: {e}");
        exit(1);
    }
}

/// Resolve the §6 env: the balls-assigned plugin name, the XDG state home (the
/// territory base, §1), the `bl` to shell back to, and the cwd (the change
/// worktree a `close.pre` recovers its id from, §7).
fn read_env() -> Env {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let state_home = env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    Env {
        plugin_name: env::var("BALLS_PLUGIN_NAME").unwrap_or_else(|_| USER_AGENT.to_string()),
        state_home,
        bl_program: env::var_os("BALLS_BL").map_or_else(|| PathBuf::from("bl"), PathBuf::from),
        cwd: env::current_dir().unwrap_or_default(),
    }
}
