//! `balls-plugin-github-issues` — the GitHub Issues mirror, ported to the §6/§7
//! subprocess protocol (bl-613d).
//!
//! A thin process edge over [`dispatch`]: it resolves the host environment once
//! (HOME/XDG, cwd, the `bl` binary, the import guard) — the bl-bfa8 "no env reads
//! in the lib" rule — and hands the rest to [`dispatch::run`]. All policy lives
//! in the handler modules; `main` only adapts the boundary.

mod adopt;
mod base;
mod config;
mod content;
mod dispatch;
mod issues_api;
mod marker;
mod pull;
mod push;
mod shellback;
mod store;
mod territory;
mod wire;

use std::env;
use std::io;
use std::path::PathBuf;
use std::process::exit;

use dispatch::Env;
use territory::Xdg;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let xdg = Xdg {
        home: env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
        state_home: env::var_os("XDG_STATE_HOME").map(PathBuf::from),
    };
    let cwd = env::current_dir().unwrap_or_default();
    let env = Env {
        xdg,
        cwd: &cwd,
        bl_bin: shellback::resolve_bin(env::var_os("BALLS_BIN")),
        importing: env::var_os(shellback::IMPORT_GUARD).is_some(),
        default_api_base: "https://api.github.com".to_string(),
    };
    let code = dispatch::run(&args, &mut io::stdin().lock(), &mut io::stdout().lock(), &env);
    exit(code);
}
