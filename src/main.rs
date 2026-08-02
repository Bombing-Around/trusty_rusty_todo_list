mod category_manager;
mod cli;
mod config;
mod models;
mod storage;

use category_manager::{CategoryManager, UNCATEGORIZED_ID};
use clap::Parser;
use cli::{CategoryCommands, Cli, Commands, ConfigCommands};
use config::{ConfigError, ConfigManager};
use models::{Category, CategoryError, StorageError};
use std::path::PathBuf;
use storage::{create_storage, Storage, StorageType};
use thiserror::Error;

/// Top-level application error type.
///
/// Wraps errors from the various subsystems (config, storage, categories, and
/// eventually tasks) so `main` has a single place to format and report
/// failures.
#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Category(#[from] CategoryError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid key=value pair '{0}': expected format key=value")]
    MalformedKeyValue(String),
    #[error("unknown storage.type '{0}': expected one of json, sqlite")]
    UnknownStorageType(String),
    #[error("could not determine the home directory; set storage.path explicitly")]
    NoHomeDirectory,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();

    // Initialize config manager, honoring an explicit --config / TRTODO_CONFIG
    // override so callers (and tests) can point at a config file outside $HOME.
    let mut config_manager = ConfigManager::new(cli.config.as_deref())?;

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
        Commands::Category { command } => {
            let storage = open_storage(&config_manager)?;
            run_category_command(&*storage, command)?;
        }
        _ => {
            println!("Command handling not yet implemented");
        }
    }

    Ok(())
}

/// Builds the task storage backend described by the current configuration.
///
/// `storage.path` is the storage *location* (a directory, per the README's
/// `~/.config/trtodo` default); the data file inside it is named after the
/// chosen backend.
fn open_storage(config_manager: &ConfigManager) -> Result<Box<dyn Storage>, CliError> {
    let storage_type = match config_manager.get("storage.type").as_deref() {
        None | Some("json") => StorageType::Json,
        Some("sqlite") => StorageType::Sqlite,
        Some(other) => return Err(CliError::UnknownStorageType(other.to_string())),
    };

    let dir = match config_manager.get("storage.path") {
        Some(path) => PathBuf::from(shellexpand::tilde(&path).as_ref()),
        None => dirs::home_dir()
            .ok_or(CliError::NoHomeDirectory)?
            .join(".config")
            .join("trtodo"),
    };

    let file_name = match storage_type {
        StorageType::Json => "trtodo-data.json",
        StorageType::Sqlite => "trtodo-data.db",
    };

    // SQLite cannot create the containing directory for us, and JSON only does
    // so on save - make sure it exists before either backend opens the file.
    std::fs::create_dir_all(&dir)?;

    Ok(create_storage(storage_type, &dir.join(file_name))?)
}

fn run_category_command(storage: &dyn Storage, command: CategoryCommands) -> Result<(), CliError> {
    let mut manager = CategoryManager::new(storage);

    match command {
        CategoryCommands::Add { name } => {
            let id = manager.add_category(name.clone(), None)?;
            println!("Category '{}' added with ID {}", name, id);
        }
        CategoryCommands::Delete { name, new_category } => {
            let category = resolve_category(&manager, &name)?;

            let new_category_id = match new_category {
                Some(ref target) => Some(resolve_category(&manager, target)?.id),
                None => None,
            };

            manager.delete_category(category.id, new_category_id)?;
            match new_category {
                Some(target) => println!(
                    "Category '{}' deleted; its tasks were moved to '{}'",
                    category.name, target
                ),
                None => println!(
                    "Category '{}' deleted; its tasks are now uncategorized",
                    category.name
                ),
            }
        }
        CategoryCommands::Update { old_name, new_name } => {
            let category = resolve_category(&manager, &old_name)?;
            manager.update_category(category.id, new_name.clone())?;
            println!("Category '{}' renamed to '{}'", category.name, new_name);
        }
        CategoryCommands::List => {
            let current = manager.get_current_category();
            println!("Categories:");
            for category in manager.list_categories()? {
                let marker = if Some(category.id) == current {
                    " (current)"
                } else {
                    ""
                };
                println!("{}: {}{}", category.id, category.name, marker);
            }
        }
        CategoryCommands::Use { category } => {
            let category = resolve_category(&manager, &category)?;
            manager.use_category(category.id)?;
            println!("Now using category '{}' ({})", category.name, category.id);
        }
        CategoryCommands::Clear => {
            manager.clear_category_context()?;
            println!("Category context cleared");
        }
        CategoryCommands::Show => match manager.get_current_category() {
            Some(UNCATEGORIZED_ID) | None => {
                println!("Current category: Uncategorized (ID: {})", UNCATEGORIZED_ID);
            }
            Some(id) => match manager.get_category(id)? {
                Some(category) => {
                    println!("Current category: {} (ID: {})", category.name, category.id);
                }
                None => println!("Current category: unknown (ID: {})", id),
            },
        },
    }

    Ok(())
}

/// Resolves a user-supplied category reference, which the README allows to be
/// either a category name or a category ID. Names take precedence.
fn resolve_category(manager: &CategoryManager, reference: &str) -> Result<Category, CliError> {
    if let Some(category) = manager.get_category_by_name(reference)? {
        return Ok(category);
    }

    if let Ok(id) = reference.parse::<u64>() {
        if let Some(category) = manager.get_category(id)? {
            return Ok(category);
        }
    }

    Err(CategoryError::NotFound(reference.to_string()).into())
}
