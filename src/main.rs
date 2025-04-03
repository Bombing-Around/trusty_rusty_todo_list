mod cli;
mod config;
mod models;
mod storage;

use clap::Parser;
use cli::{Cli, Commands, ConfigCommands};
use config::ConfigManager;

fn main() {
    let cli = Cli::parse();

    // Initialize config manager
    let mut config_manager = ConfigManager::new(None).expect("Failed to initialize config manager");

    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Set { key_value } => {
                let parts: Vec<&str> = key_value.split('=').collect();
                if parts.len() != 2 {
                    eprintln!("Invalid key-value format. Use key=value");
                    std::process::exit(1);
                }
                if let Err(e) = config_manager.set(parts[0], parts[1]) {
                    eprintln!("Failed to set config: {}", e);
                    std::process::exit(1);
                }
                println!("Configuration updated successfully");
            }
            ConfigCommands::Default { key } => {
                if let Err(e) = config_manager.unset(&key) {
                    eprintln!("Failed to reset config: {}", e);
                    std::process::exit(1);
                }
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
}
