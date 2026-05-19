mod auth;
mod cli;
mod commands;
mod config;
mod error;
mod github;
mod types;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::AuthSetup { config, auth_dir } => commands::auth_setup::run(&config, &auth_dir),
        Command::AuthCheck { config, auth_dir } => commands::auth_check::run(&config, &auth_dir),
        Command::Push {
            task,
            config,
            auth_dir,
        } => commands::push::run(&task, &config, &auth_dir),
        Command::Sync {
            task,
            config,
            auth_dir,
        } => commands::sync::run(task.as_deref(), &config, &auth_dir),
    };
    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
