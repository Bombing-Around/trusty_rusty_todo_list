use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Debug, ValueEnum, PartialEq)]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the configuration file to use instead of the default
    ///
    /// Takes precedence over the `TRTODO_CONFIG` environment variable.
    #[arg(
        long = "config",
        value_name = "PATH",
        global = true,
        env = "TRTODO_CONFIG"
    )]
    pub config: Option<PathBuf>,

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
        /// Category name or ID. Omit to search the current category context,
        /// or every category if no context is set (prompting if the title is
        /// ambiguous).
        #[arg(short = 'c', long = "category")]
        category: Option<String>,
    },
    /// Update a task
    Update {
        /// Title or ID of the task
        title_or_id: String,
        /// New title for the task
        #[arg(short = 't', long = "to")]
        new_title: String,
        /// Category name or ID. Omit to search the current category context,
        /// or every category if no context is set (prompting if the title is
        /// ambiguous).
        #[arg(short = 'c', long = "category")]
        category: Option<String>,
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
    /// Soft-deleted task management commands
    Deleted {
        #[command(subcommand)]
        command: DeletedCommands,
    },
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
        /// Name of the category
        name: String,
        /// New category for tasks (optional)
        #[arg(short = 'n', long = "new-category")]
        new_category: Option<String>,
    },
    /// Update a category name
    Update {
        /// Old category name
        old_name: String,
        /// New category name
        new_name: String,
    },
    /// List all categories
    List,
}

/// The `deleted` namespace: everything you can do with the tasks that
/// `trtodo delete` soft-deleted (issue #29's `deleted_at` timestamp).
///
/// `list` and `restore` were previously absent on the grounds that the
/// README documented only `flush`; issues #31 and #32 settled that surface
/// and the README now documents all three. They are not optional extras:
/// soft-deleted tasks are hidden from `list` and `search`, so without
/// `deleted list` they are invisible right up until `flush` destroys them,
/// and without `deleted restore` a soft delete is just a slower hard delete.
#[derive(Subcommand)]
pub enum DeletedCommands {
    /// List all soft-deleted tasks, showing what a flush would destroy
    List,
    /// Restore a soft-deleted task to its original category
    Restore {
        /// Title or ID of the soft-deleted task
        ///
        /// Resolved among soft-deleted tasks only, so this can never
        /// accidentally match a live task. There is no `--category`: run
        /// `deleted list` to see the IDs.
        title_or_id: String,
    },
    /// Permanently remove all soft-deleted tasks
    Flush {
        /// Skip the confirmation prompt
        ///
        /// Required when running non-interactively (piped stdin, CI,
        /// scripts): with no terminal to confirm at, `flush` refuses rather
        /// than destroying data unattended.
        #[arg(short = 'y', long = "yes", visible_alias = "force")]
        yes: bool,
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
    fn test_config_path_override() {
        // Accepted before the subcommand.
        let cli = parse_args(&["trtodo", "--config", "/tmp/x.json", "config", "list"]);
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/x.json")));

        // And after it, since the argument is global.
        let cli = parse_args(&["trtodo", "config", "list", "--config", "/tmp/x.json"]);
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/x.json")));

        // Absent by default, so ConfigManager falls back to the $HOME location.
        let cli = parse_args(&["trtodo", "config", "list"]);
        assert_eq!(cli.config, None);
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
    fn test_deleted_commands() {
        // Test deleted list
        let cli = parse_args(&["trtodo", "deleted", "list"]);
        match cli.command {
            Commands::Deleted { command } => match command {
                DeletedCommands::List => {}
                _ => panic!("Expected Deleted List command"),
            },
            _ => panic!("Expected Deleted command"),
        }

        // Test deleted restore
        let cli = parse_args(&["trtodo", "deleted", "restore", "Buy milk"]);
        match cli.command {
            Commands::Deleted { command } => match command {
                DeletedCommands::Restore { title_or_id } => {
                    assert_eq!(title_or_id, "Buy milk");
                }
                _ => panic!("Expected Deleted Restore command"),
            },
            _ => panic!("Expected Deleted command"),
        }

        // Test deleted flush: confirmation is opt-out, so `yes` is false
        // unless the escape hatch was passed.
        let cli = parse_args(&["trtodo", "deleted", "flush"]);
        match cli.command {
            Commands::Deleted { command } => match command {
                DeletedCommands::Flush { yes } => assert!(!yes),
                _ => panic!("Expected Deleted Flush command"),
            },
            _ => panic!("Expected Deleted command"),
        }

        // ... in any of its three spellings.
        for flag in ["--yes", "-y", "--force"] {
            let cli = parse_args(&["trtodo", "deleted", "flush", flag]);
            match cli.command {
                Commands::Deleted { command } => match command {
                    DeletedCommands::Flush { yes } => assert!(yes, "{flag} should confirm"),
                    _ => panic!("Expected Deleted Flush command"),
                },
                _ => panic!("Expected Deleted command"),
            }
        }
    }

    #[test]
    fn test_deleted_restore_requires_a_task_reference() {
        // Nothing sensible to restore without one, and defaulting to "all"
        // would be a surprising thing to do by accident.
        assert!(try_parse_args(&["trtodo", "deleted", "restore"]).is_err());
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
