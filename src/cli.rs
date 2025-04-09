use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum, PartialEq)]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Add a new task
    Add {
        /// Title of the task
        title: String,
        /// Category name or ID
        #[arg(short = 'c', long = "category")]
        category: String,
        /// Priority level
        #[arg(short = 'p', long = "priority")]
        priority: Option<Priority>,
    },
    /// Delete a task
    Delete {
        /// Title or ID of the task
        title_or_id: String,
        /// Category name or ID
        #[arg(short = 'c', long = "category")]
        category: String,
    },
    /// Update a task
    Update {
        /// Title or ID of the task
        title_or_id: String,
        /// New title for the task
        #[arg(short = 't', long = "to")]
        new_title: String,
        /// Category name or ID
        #[arg(short = 'c', long = "category")]
        category: String,
    },
    /// Check off a task
    #[command(alias = "x", alias = "mark")]
    Check {
        /// Title or ID of the task
        title_or_id: String,
        /// Category name or ID
        #[arg(short = 'c', long = "category")]
        category: Option<String>,
    },
    /// Uncheck a task
    #[command(alias = "o", alias = "unmark")]
    Uncheck {
        /// Title or ID of the task
        title_or_id: String,
        /// Category name or ID
        #[arg(short = 'c', long = "category")]
        category: Option<String>,
    },
    /// Check off all tasks in current category
    CheckAll,
    /// Uncheck all tasks in current category
    UncheckAll,
    /// Move a task to another category
    Move {
        /// Task name or ID (optional for extended syntax)
        task_name_or_id: Option<String>,
        /// Target category name or ID
        #[arg(short = 't', long = "to")]
        to_category: Option<String>,
        /// Source category name or ID (for extended syntax)
        #[arg(long = "from")]
        from_category: Option<String>,
        /// Task name or ID (for extended syntax)
        #[arg(long = "task")]
        task: Option<String>,
    },
    /// List all tasks
    List {
        /// Search term to filter tasks
        #[arg(short = 's', long = "search")]
        search: Option<String>,
        /// Show completed tasks
        #[arg(short = 'c', long = "completed")]
        completed: bool,
        /// Filter by priority
        #[arg(short = 'p', long = "priority")]
        priority: Option<Priority>,
    },
    /// Category management commands
    Category {
        #[command(subcommand)]
        command: CategoryCommands,
    },
    /// Configuration management commands
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Flush deleted items
    Flush,
}

#[derive(Subcommand)]
pub enum MoveCommands {
    /// Move task using simple syntax
    #[command(name = "")]
    Simple {
        /// Task name or ID
        task_name_or_id: String,
        /// Target category name or ID
        #[arg(short = 't', long = "to")]
        to_category: String,
    },
    /// Move task using extended syntax
    #[command(name = "from")]
    Extended {
        /// Source category name or ID
        #[arg(long = "from")]
        from_category: String,
        /// Target category name or ID (optional, omit to move to uncategorized)
        #[arg(long = "to")]
        to_category: Option<String>,
        /// Task name or ID
        #[arg(long = "task")]
        task_name_or_id: String,
    },
}

#[derive(Subcommand)]
pub enum CategoryCommands {
    /// Set current category context
    Use {
        /// Category name or ID
        #[arg(help = "Category name or ID (e.g. 'Home' or '1')")]
        category: String,
    },
    /// Clear current category context
    Clear,
    /// Show current category context
    Show,
    /// Add a new category
    Add {
        /// Name of the category
        name: String,
    },
    /// Delete a category
    Delete {
        /// Name or ID of the category
        #[arg(help = "Category name or ID (e.g. 'Home' or '1')")]
        name_or_id: String,
        /// New category for tasks (optional)
        #[arg(short = 'n', long = "new-category", help = "Category name or ID to move tasks to")]
        new_category: Option<String>,
    },
    /// Update a category name
    Update {
        /// Old category name or ID
        #[arg(help = "Category name or ID to rename")]
        old_name: String,
        /// New category name
        new_name: String,
    },
    /// List all categories
    List,
    /// Set the order of a category
    Order {
        /// Category name or ID to reorder
        #[arg(help = "Category name or ID to set order for")]
        category: String,
        /// New order position (0-based)
        #[arg(help = "New position in the order (0-based)")]
        position: u32,
    },
    /// Reorder multiple categories
    Reorder {
        /// List of category names or IDs in desired order
        #[arg(help = "Space-separated list of category names or IDs in desired order")]
        categories: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Set a configuration value
    Set {
        /// Key-value pair in format key=value
        key_value: String,
    },
    /// Reset a configuration value to default
    Default {
        /// Configuration key
        key: String,
    },
    /// Reset the database to its initial state
    Reset,
    /// List all configuration values
    List,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    fn try_parse_args(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    #[test]
    fn test_add_task() {
        let cli = parse_args(&["trtodo", "add", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Add {
                title,
                category,
                priority,
            } => {
                assert_eq!(title, "Buy milk");
                assert_eq!(category, "Home");
                assert!(priority.is_none());
            }
            _ => panic!("Expected Add command"),
        }

        // Test with priority
        let cli = parse_args(&[
            "trtodo",
            "add",
            "Buy milk",
            "--category",
            "Home",
            "--priority",
            "high",
        ]);
        match cli.command {
            Commands::Add {
                title,
                category,
                priority,
            } => {
                assert_eq!(title, "Buy milk");
                assert_eq!(category, "Home");
                assert_eq!(priority, Some(Priority::High));
            }
            _ => panic!("Expected Add command"),
        }
    }

    #[test]
    fn test_list_tasks() {
        // Test basic list
        let cli = parse_args(&["trtodo", "list"]);
        match cli.command {
            Commands::List {
                search,
                completed,
                priority,
            } => {
                assert!(search.is_none());
                assert!(!completed);
                assert!(priority.is_none());
            }
            _ => panic!("Expected List command"),
        }

        // Test list with all options
        let cli = parse_args(&[
            "trtodo",
            "list",
            "--search",
            "milk",
            "--completed",
            "--priority",
            "low",
        ]);
        match cli.command {
            Commands::List {
                search,
                completed,
                priority,
            } => {
                assert_eq!(search, Some("milk".to_string()));
                assert!(completed);
                assert_eq!(priority, Some(Priority::Low));
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_category_commands() {
        // Test category use
        let cli = parse_args(&["trtodo", "category", "use", "Home"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Use { category } => {
                    assert_eq!(category, "Home");
                }
                _ => panic!("Expected Category Use command"),
            },
            _ => panic!("Expected Category command"),
        }

        // Test category list
        let cli = parse_args(&["trtodo", "category", "list"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::List => {}
                _ => panic!("Expected Category List command"),
            },
            _ => panic!("Expected Category command"),
        }
    }

    #[test]
    fn test_config_commands() {
        // Test config set
        let cli = parse_args(&["trtodo", "config", "set", "storage.type=json"]);
        match cli.command {
            Commands::Config { command } => match command {
                ConfigCommands::Set { key_value } => {
                    assert_eq!(key_value, "storage.type=json");
                }
                _ => panic!("Expected Config Set command"),
            },
            _ => panic!("Expected Config command"),
        }

        // Test config reset
        let cli = parse_args(&["trtodo", "config", "reset"]);
        match cli.command {
            Commands::Config { command } => match command {
                ConfigCommands::Reset => {},
                _ => panic!("Expected Config Reset command"),
            },
            _ => panic!("Expected Config command"),
        }

        // Test config list
        let cli = parse_args(&["trtodo", "config", "list"]);
        match cli.command {
            Commands::Config { command } => match command {
                ConfigCommands::List => {}
                _ => panic!("Expected Config List command"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_required_arguments() {
        // Test that category is required for add command
        let result = try_parse_args(&["trtodo", "add", "Buy milk"]);
        assert!(result.is_err());

        // Test that priority must be valid
        let result = try_parse_args(&[
            "trtodo",
            "add",
            "Buy milk",
            "--category",
            "Home",
            "--priority",
            "invalid",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_command_aliases() {
        // Test 'x' alias for check
        let cli = parse_args(&["trtodo", "x", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Check {
                title_or_id,
                category,
            } => {
                assert_eq!(title_or_id, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
            }
            _ => panic!("Expected Check command"),
        }

        // Test 'mark' alias for check
        let cli = parse_args(&["trtodo", "mark", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Check {
                title_or_id,
                category,
            } => {
                assert_eq!(title_or_id, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
            }
            _ => panic!("Expected Check command"),
        }

        // Test 'o' alias for uncheck
        let cli = parse_args(&["trtodo", "o", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Uncheck {
                title_or_id,
                category,
            } => {
                assert_eq!(title_or_id, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
            }
            _ => panic!("Expected Uncheck command"),
        }

        // Test 'unmark' alias for uncheck
        let cli = parse_args(&["trtodo", "unmark", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Uncheck {
                title_or_id,
                category,
            } => {
                assert_eq!(title_or_id, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
            }
            _ => panic!("Expected Uncheck command"),
        }
    }

    #[test]
    fn test_move_commands() {
        // Test simple move syntax
        let cli = parse_args(&["trtodo", "move", "Buy milk", "--to", "Shopping"]);
        match cli.command {
            Commands::Move {
                task_name_or_id,
                to_category,
                from_category,
                task,
            } => {
                assert_eq!(task_name_or_id, Some("Buy milk".to_string()));
                assert_eq!(to_category, Some("Shopping".to_string()));
                assert!(from_category.is_none());
                assert!(task.is_none());
            }
            _ => panic!("Expected Move command"),
        }

        // Test extended move syntax
        let cli = parse_args(&[
            "trtodo", "move", "--from", "Home", "--to", "Shopping", "--task", "Buy milk",
        ]);
        match cli.command {
            Commands::Move {
                task_name_or_id,
                to_category,
                from_category,
                task,
            } => {
                assert!(task_name_or_id.is_none());
                assert_eq!(to_category, Some("Shopping".to_string()));
                assert_eq!(from_category, Some("Home".to_string()));
                assert_eq!(task, Some("Buy milk".to_string()));
            }
            _ => panic!("Expected Move command"),
        }

        // Test extended move syntax without target category (move to uncategorized)
        let cli = parse_args(&["trtodo", "move", "--from", "Home", "--task", "Buy milk"]);
        match cli.command {
            Commands::Move {
                task_name_or_id,
                to_category,
                from_category,
                task,
            } => {
                assert!(task_name_or_id.is_none());
                assert!(to_category.is_none());
                assert_eq!(from_category, Some("Home".to_string()));
                assert_eq!(task, Some("Buy milk".to_string()));
            }
            _ => panic!("Expected Move command"),
        }
    }
}
