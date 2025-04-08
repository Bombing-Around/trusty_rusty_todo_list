mod category_manager;
mod cli;
mod config;
mod models;
mod storage;

use clap::Parser;
use cli::{Cli, Commands, ConfigCommands, CategoryCommands};
use config::ConfigManager;
use category_manager::CategoryManager;
use crate::models::Category;
use std::process;

fn initialize_default_categories(storage: &dyn storage::Storage) -> Result<(), Box<dyn std::error::Error>> {
    let mut data = storage.load()?;
    if data.categories.is_empty() {
        // Add default categories
        let mut home = Category::new("Home".to_string(), Some("Home tasks".to_string()))?;
        let mut work = Category::new("Work".to_string(), Some("Work tasks".to_string()))?;
        
        // Set IDs for default categories
        home.id = 1;
        work.id = 2;
        
        data.categories.push(home);
        data.categories.push(work);
        storage.save(&data)?;
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    // Initialize config manager
    let mut config_manager = ConfigManager::new(None).expect("Failed to initialize config manager");
    let storage = config_manager.get_storage();
    
    // Initialize default categories on first run
    if let Err(e) = initialize_default_categories(&*storage) {
        eprintln!("Failed to initialize default categories: {}", e);
        process::exit(1);
    }

    let mut category_manager = CategoryManager::new(&*storage);

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
            ConfigCommands::Reset => {
                println!("Warning: This will delete all tasks and categories.");
                println!("The database will be reset to its initial state with default categories.");
                println!("Are you sure you want to continue? [y/N]");
                
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_err() {
                    eprintln!("Failed to read input");
                    std::process::exit(1);
                }

                if input.trim().to_lowercase() != "y" {
                    println!("Operation cancelled");
                    return;
                }

                // Create a fresh empty data state
                let data = crate::models::StorageData::new();
                
                // Save the empty data state
                if let Err(e) = storage.save(&data) {
                    eprintln!("Failed to reset database: {}", e);
                    std::process::exit(1);
                }
                
                // Reinitialize with default categories
                if let Err(e) = initialize_default_categories(&*storage) {
                    eprintln!("Failed to initialize default categories: {}", e);
                    std::process::exit(1);
                }
                
                println!("Database has been reset to initial state with default categories");
            }
            ConfigCommands::List => {
                let configs = config_manager.list();
                for (key, value, is_default) in configs {
                    println!("{}{} = {}", if is_default { "*" } else { " " }, key, value);
                }
            }
        },
        Commands::Category { command } => match command {
            CategoryCommands::Add { name } => {
                match category_manager.add_category(name.clone(), None) {
                    Ok(id) => println!("Category '{}' added with ID {}", name, id),
                    Err(e) => {
                        eprintln!("Failed to add category: {}", e);
                        process::exit(1);
                    }
                }
            },
            CategoryCommands::Delete { name_or_id, new_category } => {
                // Try to get category by name or ID
                let category = if let Ok(id) = name_or_id.parse::<u64>() {
                    // Try to get by ID first
                    match category_manager.get_category(id) {
                        Ok(Some(c)) => c,
                        Ok(None) => {
                            eprintln!("Category with ID {} not found", id);
                            process::exit(1);
                        },
                        Err(e) => {
                            eprintln!("Error finding category: {}", e);
                            process::exit(1);
                        }
                    }
                } else {
                    // Try to get by name
                    match category_manager.get_category_by_name(&name_or_id) {
                        Ok(Some(c)) => c,
                        Ok(None) => {
                            eprintln!("Category '{}' not found", name_or_id);
                            process::exit(1);
                        },
                        Err(e) => {
                            eprintln!("Error finding category: {}", e);
                            process::exit(1);
                        }
                    }
                };

                // If new_category is specified, get its ID
                let new_category_id = if let Some(new_cat) = new_category {
                    match category_manager.get_category_by_name(&new_cat) {
                        Ok(Some(c)) => Some(c.id),
                        Ok(None) => {
                            eprintln!("New category '{}' not found", new_cat);
                            process::exit(1);
                        },
                        Err(e) => {
                            eprintln!("Error finding new category: {}", e);
                            process::exit(1);
                        }
                    }
                } else {
                    None
                };

                match category_manager.delete_category(category.id, new_category_id) {
                    Ok(_) => println!("Category '{}' deleted", category.name),
                    Err(e) => {
                        eprintln!("Failed to delete category: {}", e);
                        process::exit(1);
                    }
                }
            },
            CategoryCommands::Update { old_name, new_name } => {
                // First try to get category by name
                let category = match category_manager.get_category_by_name(&old_name) {
                    Ok(Some(c)) => c,
                    Ok(None) => {
                        eprintln!("Category '{}' not found", old_name);
                        process::exit(1);
                    },
                    Err(e) => {
                        eprintln!("Error finding category: {}", e);
                        process::exit(1);
                    }
                };

                match category_manager.update_category(category.id, new_name.clone()) {
                    Ok(_) => println!("Category '{}' renamed to '{}'", old_name, new_name),
                    Err(e) => {
                        eprintln!("Failed to update category: {}", e);
                        process::exit(1);
                    }
                }
            },
            CategoryCommands::List => {
                match category_manager.list_categories() {
                    Ok(categories) => {
                        println!("Categories:");
                        for category in categories {
                            println!("{}: {} {}", category.id, category.name, 
                                if Some(category.id) == category_manager.get_current_category() {
                                    "(current)"
                                } else {
                                    ""
                                }
                            );
                        }
                    },
                    Err(e) => {
                        eprintln!("Failed to list categories: {}", e);
                        process::exit(1);
                    }
                }
            },
            CategoryCommands::Use { category } => {
                // Try to get category by name first
                let category_id = match category_manager.get_category_by_name(&category) {
                    Ok(Some(c)) => c.id,
                    Ok(None) => {
                        // If not found by name, try parsing as ID
                        match category.parse::<u64>() {
                            Ok(id) => id,
                            Err(_) => {
                                eprintln!("Category '{}' not found", category);
                                process::exit(1);
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("Error finding category: {}", e);
                        process::exit(1);
                    }
                };

                // Get the category name before setting it as current
                let category_name = match category_manager.get_category(category_id) {
                    Ok(Some(cat)) => cat.name,
                    Ok(None) => {
                        eprintln!("Category with ID {} not found", category_id);
                        process::exit(1);
                    },
                    Err(e) => {
                        eprintln!("Error finding category: {}", e);
                        process::exit(1);
                    }
                };

                match category_manager.use_category(category_id) {
                    Ok(_) => println!("Now using category '{}' ({})", category_name, category_id),
                    Err(e) => {
                        eprintln!("Failed to set category context: {}", e);
                        process::exit(1);
                    }
                }
            },
            CategoryCommands::Clear => {
                match category_manager.clear_category_context() {
                    Ok(_) => println!("Category context cleared"),
                    Err(e) => {
                        eprintln!("Failed to clear category context: {}", e);
                        process::exit(1);
                    }
                };
            },
            CategoryCommands::Show => {
                match category_manager.get_current_category() {
                    Some(id) => {
                        match category_manager.get_category(id) {
                            Ok(Some(category)) => println!("Current category: {}", category.name),
                            Ok(None) => println!("Current category ID {} not found", id),
                            Err(e) => {
                                eprintln!("Error getting current category: {}", e);
                                process::exit(1);
                            }
                        }
                    },
                    None => println!("No category context set")
                }
            },
        },
        _ => {
            println!("Command handling not yet implemented");
        }
    }
}
