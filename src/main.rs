mod cli;
mod config;
mod models;
mod storage;

use clap::Parser;
use cli::{Cli, Commands, ConfigCommands};
use config::{ConfigError, ConfigManager};
use thiserror::Error;

/// Top-level application error type.
///
/// Wraps errors from the various subsystems (config, and eventually
/// storage/task/category once those commands are wired up) so `main` has a
/// single place to format and report failures.
#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("invalid key=value pair '{0}': expected format key=value")]
    MalformedKeyValue(String),
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();

    // Initialize config manager
    let mut config_manager = ConfigManager::new(None)?;

    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Set { key_value } => {
                let (key, value) = key_value
                    .split_once('=')
                    .ok_or_else(|| CliError::MalformedKeyValue(key_value.clone()))?;
                config_manager.set(key, value)?;
                println!("Configuration updated successfully");
            }
            ConfigCommands::Default { key } => {
                config_manager.unset(&key)?;
                println!("Configuration reset to default");
            }
            ConfigCommands::List => {
                let configs = config_manager.list();
                for (key, value, is_default) in configs {
                    println!("{}{} = {}", if is_default { "*" } else { " " }, key, value);
                }
            }
        },
        _ => {
            println!("Command handling not yet implemented");
        }
    }

    Ok(())
}
