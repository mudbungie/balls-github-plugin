use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "balls-plugin-github",
    about = "GitHub forge plugin for balls (deferred-mode delivery via pull requests)"
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

    /// Open or update the PR for a deferred-mode review task (Task JSON on stdin).
    Push {
        #[arg(long, value_name = "ID")]
        task: String,
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        auth_dir: PathBuf,
    },

    /// Poll open PRs; close the gate child of any whose PR has merged
    /// (all tasks JSON on stdin).
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
