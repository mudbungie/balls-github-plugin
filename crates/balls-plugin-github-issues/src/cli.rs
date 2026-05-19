use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "balls-plugin-github-issues",
    about = "GitHub Issues plugin for balls (bidirectional task mirror)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Read a GitHub token from stdin, validate it, and store it in --auth-dir.
    #[command(name = "auth-setup")]
    AuthSetup {
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        auth_dir: PathBuf,
    },

    /// Verify the stored token. Exit 0 if valid, non-zero otherwise.
    #[command(name = "auth-check")]
    AuthCheck {
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        auth_dir: PathBuf,
    },

    /// Mirror balls-side lifecycle changes to GitHub Issues (Task JSON
    /// on stdin). B1 lands a silent noop; B3 wires the actual mirror.
    Push {
        #[arg(long, value_name = "ID")]
        task: String,
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        auth_dir: PathBuf,
    },

    /// Poll GitHub Issues and emit a SyncReport for external changes
    /// (all tasks JSON on stdin). B1 lands a silent noop; B4a–d wire
    /// classification + per-entry-kind emission.
    Sync {
        #[arg(long, value_name = "ID")]
        task: Option<String>,
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        auth_dir: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_valid() {
        Cli::command().debug_assert();
    }
}
