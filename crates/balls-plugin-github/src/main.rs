//! `balls-plugin-github` — the GitHub forge plugin, a §6/§7 subprocess plugin
//! on the SUBTASK model (bl-7bfe): the review gate is an ordinary close-blocker
//! gate child, NOT a delivery variant.
//!
//! It is wired ALONGSIDE the worktree-owning `bl-delivery` (which keeps every
//! delivery hook — there is no forge `close.pre`); this plugin touches exactly
//! two moments:
//!
//! ```text
//! claim.post = ["bl-delivery", "balls-plugin-github", "bl-tracker"]  # worktree, then mint the review gate child
//! sync.post  = ["balls-plugin-github"]                            # close the gate child on PR merge
//! ```
//!
//! Submission is GIT-NATIVE WORK: the worker pushes `work/<id>` and opens the
//! PR themselves with `[bl-id]` in the title; the merged squash is what core
//! delivery's tag-scan (bl-430e) recognizes, so the parent's `bl close` skips
//! the local squash. See [`forge`] for the full matrix and the rollback shape.
//!
//! `main` only adapts the process boundary: it reads the §6 env ONCE here (the
//! bl-bfa8 rule — no env reads in the library modules), slurps stdin (the §7
//! wire, or the token for `auth-setup`), and hands off to [`edge::handle`].

mod authcmd;
mod bl_ops;
mod edge;
mod forge;
mod pr_api;
mod project;
mod wire;

use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::exit;

/// The plugin's own name — the User-Agent it sends, and the territory/config/
/// join-key default when balls does not name it via `BALLS_PLUGIN_NAME`.
pub const USER_AGENT: &str = "balls-plugin-github";

/// The process-edge environment the §6 dispatch needs, read ONCE in [`main`].
pub struct Env {
    pub plugin_name: String,
    pub state_home: PathBuf,
    pub bl_program: PathBuf,
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

/// Resolve the §6 env: the balls-assigned plugin name (also the join key the
/// gate children carry) and the XDG state home (the auth territory base, §1).
/// `bl` is resolved on `$PATH` — core sets only BALLS_PROTOCOL/PLUGIN_NAME/DEPTH
/// (§6/§7), so a plugin shells `bl` off the triggering invocation's env, exactly
/// like bl-chore and bl-tracker. `bl_program` stays a field so unit tests inject
/// a fake binary through the [`Bl`] constructor without mutating global env.
fn read_env() -> Env {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let state_home = env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    Env {
        plugin_name: env::var("BALLS_PLUGIN_NAME").unwrap_or_else(|_| USER_AGENT.to_string()),
        state_home,
        bl_program: PathBuf::from("bl"),
    }
}
