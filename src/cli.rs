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
    /// Takes precedence over the `TRT_CONFIG` environment variable.
    #[arg(
        long = "config",
        value_name = "PATH",
        global = true,
        env = "TRT_CONFIG"
    )]
    pub config: Option<PathBuf>,

    // These two are the non-interactive escape hatch for scripts and CI:
    // they let an invocation answer the first-run offer of the
    // default categories without a terminal, so automation is never blocked
    // waiting on input. Questions with no yes/no answer - picking between
    // several tasks that share a name - are still refused under both flags
    // rather than guessed at; see `prompter::NonInteractivePrompter`.
    /// Assume "yes" for confirmation prompts and never read from stdin
    ///
    /// Accepts the first-run offer to create the default categories without
    /// asking. Questions that aren't yes/no (such as which of several
    /// same-named tasks you meant) are still refused rather than guessed at.
    #[arg(long = "yes", short = 'y', global = true, conflicts_with = "no_input")]
    pub yes: bool,

    /// Never prompt; decline confirmations and never read from stdin
    ///
    /// The counterpart to --yes. Declining the first-run offer is a real
    /// answer, so it is remembered and never asked again.
    #[arg(long = "no-input", global = true)]
    pub no_input: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Add a new task
    Add {
        /// Title of the task
        title: String,
        /// Category name or ID. Omit to use the current category context, then
        /// the `default-category` config value, then Uncategorized.
        #[arg(short = 'c', long = "category")]
        category: Option<String>,
        /// Priority level
        #[arg(short = 'p', long = "priority")]
        priority: Option<Priority>,
        /// Description of the task
        #[arg(short = 'd', long = "description")]
        description: Option<String>,
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
    ///
    /// At least one of `--to`, `--description`, or `--clear-description`
    /// must be given - there is otherwise nothing to update.
    Update {
        /// Title or ID of the task
        title_or_id: String,
        /// New title for the task. Omit to leave the title unchanged and
        /// update only the description.
        #[arg(short = 't', long = "to")]
        new_title: Option<String>,
        /// Category name or ID. Omit to search the current category context,
        /// or every category if no context is set (prompting if the title is
        /// ambiguous).
        #[arg(short = 'c', long = "category")]
        category: Option<String>,
        /// New description for the task
        #[arg(
            short = 'd',
            long = "description",
            conflicts_with = "clear_description"
        )]
        description: Option<String>,
        /// Clear the task's description back to empty
        ///
        /// Explicit rather than inferred from an empty `--description ""`,
        /// so a description is never cleared by accident.
        #[arg(long = "clear-description")]
        clear_description: bool,
    },
    /// Show full details of a task, including its description
    Show {
        /// Title or ID of the task
        title_or_id: String,
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
    /// List tasks, narrowed to the current category context if one is set
    ///
    /// Unlike `Delete`/`Update`/`Check`/`Uncheck`, the category flag here has
    /// no short form: `-c` is already `--completed` on this command, so
    /// letter-for-letter consistency with the other task commands loses to
    /// not silently repurposing an existing flag.
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
        /// Category name or ID. Omit to use the current category context if
        /// one is set, else every category
        #[arg(long = "category", conflicts_with = "all")]
        category: Option<String>,
        /// List every category, ignoring the current category context
        #[arg(long = "all")]
        all: bool,
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
        /// Description of the category
        #[arg(short = 'd', long = "description")]
        description: Option<String>,
    },
    /// Delete a category
    Delete {
        /// Name of the category
        name: String,
        /// New category for tasks (optional)
        #[arg(short = 'n', long = "new-category")]
        new_category: Option<String>,
    },
    /// Update a category's name and/or description
    ///
    /// At least one of `new_name`, `--description`, or `--clear-description`
    /// must be given - there is otherwise nothing to update.
    Update {
        /// Old category name
        old_name: String,
        /// New category name. Omit to leave the name unchanged and update
        /// only the description.
        new_name: Option<String>,
        /// New description for the category
        #[arg(
            short = 'd',
            long = "description",
            conflicts_with = "clear_description"
        )]
        description: Option<String>,
        /// Clear the category's description back to empty
        ///
        /// Explicit rather than inferred from an empty `--description ""`,
        /// so a description is never cleared by accident.
        #[arg(long = "clear-description")]
        clear_description: bool,
    },
    /// List all categories
    List,
    /// Move a category to a specific position in `category list`
    ///
    /// Positions are 1-based, matching every other user-facing identifier
    /// in this CLI (task and category IDs both start at 1). Position `0`
    /// is refused: it is the fixed, unassignable position the synthesized
    /// "Uncategorized" category always sorts at, and `Uncategorized`
    /// itself cannot be targeted at all, since it is never a real,
    /// storable category.
    Order {
        /// Category name or ID
        category: String,
        /// 1-based position to move it to
        position: u32,
    },
    /// Set the order of several categories at once, in the order given
    ///
    /// Categories not listed keep whatever order they already had, rather
    /// than being pushed after the ones just reordered - list every
    /// category to control the full result. `Uncategorized` cannot appear
    /// in the list, for the same reason it cannot be targeted by
    /// `category order`.
    Reorder {
        /// Category names or IDs, listed in the order they should appear
        #[arg(required = true)]
        categories: Vec<String>,
    },
}

/// The `deleted` namespace: everything you can do with the tasks that
/// `trt delete` soft-deleted (the `deleted_at` timestamp).
///
/// `list` and `restore` were previously absent on the grounds that the
/// README documented only `flush`; the README now documents all three.
/// They are not optional extras:
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
        let cli = parse_args(&["trt", "--config", "/tmp/x.json", "config", "list"]);
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/x.json")));

        // And after it, since the argument is global.
        let cli = parse_args(&["trt", "config", "list", "--config", "/tmp/x.json"]);
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/x.json")));

        // Absent by default, so ConfigManager falls back to the $HOME location.
        let cli = parse_args(&["trt", "config", "list"]);
        assert_eq!(cli.config, None);
    }

    /// The non-interactive flags for the first-run offer. Both are
    /// global (usable before or after the subcommand) and mutually
    /// exclusive - "assume yes" and "assume no" cannot both hold.
    #[test]
    fn test_non_interactive_flags() {
        let cli = parse_args(&["trt", "--yes", "list"]);
        assert!(cli.yes);
        assert!(!cli.no_input);

        let cli = parse_args(&["trt", "list", "-y"]);
        assert!(cli.yes);

        let cli = parse_args(&["trt", "list", "--no-input"]);
        assert!(cli.no_input);
        assert!(!cli.yes);

        let cli = parse_args(&["trt", "list"]);
        assert!(!cli.yes);
        assert!(!cli.no_input);

        assert!(try_parse_args(&["trt", "--yes", "--no-input", "list"]).is_err());
    }

    #[test]
    fn test_add_task() {
        let cli = parse_args(&["trt", "add", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Add {
                title,
                category,
                priority,
                description,
            } => {
                assert_eq!(title, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
                assert!(priority.is_none());
                assert!(description.is_none());
            }
            _ => panic!("Expected Add command"),
        }

        // `--category` is optional: omitting it parses cleanly and
        // leaves resolution to `main::resolve_add_category`.
        let cli = parse_args(&["trt", "add", "Buy milk"]);
        match cli.command {
            Commands::Add {
                title,
                category,
                priority,
                description,
            } => {
                assert_eq!(title, "Buy milk");
                assert!(category.is_none());
                assert!(priority.is_none());
                assert!(description.is_none());
            }
            _ => panic!("Expected Add command"),
        }

        // Test with priority and description
        let cli = parse_args(&[
            "trt",
            "add",
            "Buy milk",
            "--category",
            "Home",
            "--priority",
            "high",
            "--description",
            "2%, not whole",
        ]);
        match cli.command {
            Commands::Add {
                title,
                category,
                priority,
                description,
            } => {
                assert_eq!(title, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
                assert_eq!(priority, Some(Priority::High));
                assert_eq!(description, Some("2%, not whole".to_string()));
            }
            _ => panic!("Expected Add command"),
        }
    }

    #[test]
    fn test_update_task() {
        // The new title alone.
        let cli = parse_args(&["trt", "update", "Buy milk", "--to", "Buy oat milk"]);
        match cli.command {
            Commands::Update {
                title_or_id,
                new_title,
                category,
                description,
                clear_description,
            } => {
                assert_eq!(title_or_id, "Buy milk");
                assert_eq!(new_title, Some("Buy oat milk".to_string()));
                assert!(category.is_none());
                assert!(description.is_none());
                assert!(!clear_description);
            }
            _ => panic!("Expected Update command"),
        }

        // `--to` is optional: a description-only update parses cleanly.
        let cli = parse_args(&["trt", "update", "Buy milk", "--description", "Unsweetened"]);
        match cli.command {
            Commands::Update {
                new_title,
                description,
                ..
            } => {
                assert!(new_title.is_none());
                assert_eq!(description, Some("Unsweetened".to_string()));
            }
            _ => panic!("Expected Update command"),
        }

        // `--clear-description` needs no value.
        let cli = parse_args(&["trt", "update", "Buy milk", "--clear-description"]);
        match cli.command {
            Commands::Update {
                clear_description, ..
            } => assert!(clear_description),
            _ => panic!("Expected Update command"),
        }

        // `--description` and `--clear-description` contradict each other.
        assert!(try_parse_args(&[
            "trt",
            "update",
            "Buy milk",
            "--description",
            "x",
            "--clear-description",
        ])
        .is_err());
    }

    #[test]
    fn test_show_task() {
        let cli = parse_args(&["trt", "show", "Buy milk", "--category", "Home"]);
        match cli.command {
            Commands::Show {
                title_or_id,
                category,
            } => {
                assert_eq!(title_or_id, "Buy milk");
                assert_eq!(category, Some("Home".to_string()));
            }
            _ => panic!("Expected Show command"),
        }
    }

    #[test]
    fn test_list_tasks() {
        // Test basic list
        let cli = parse_args(&["trt", "list"]);
        match cli.command {
            Commands::List {
                search,
                completed,
                priority,
                category,
                all,
            } => {
                assert!(search.is_none());
                assert!(!completed);
                assert!(priority.is_none());
                assert!(category.is_none());
                assert!(!all);
            }
            _ => panic!("Expected List command"),
        }

        // Test list with all options
        let cli = parse_args(&[
            "trt",
            "list",
            "--search",
            "milk",
            "--completed",
            "--priority",
            "low",
            "--category",
            "Home",
        ]);
        match cli.command {
            Commands::List {
                search,
                completed,
                priority,
                category,
                all,
            } => {
                assert_eq!(search, Some("milk".to_string()));
                assert!(completed);
                assert_eq!(priority, Some(Priority::Low));
                assert_eq!(category, Some("Home".to_string()));
                assert!(!all);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_list_tasks_all_flag() {
        let cli = parse_args(&["trt", "list", "--all"]);
        match cli.command {
            Commands::List { category, all, .. } => {
                assert!(category.is_none());
                assert!(all);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_list_tasks_category_and_all_conflict() {
        // clap enforces this at parse time so `main` never has to reconcile
        // "narrow to this category" and "ignore the context" at once.
        let result = Cli::try_parse_from(["trt", "list", "--category", "Home", "--all"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_category_commands() {
        // Test category use
        let cli = parse_args(&["trt", "category", "use", "Home"]);
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
        let cli = parse_args(&["trt", "category", "list"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::List => {}
                _ => panic!("Expected Category List command"),
            },
            _ => panic!("Expected Category command"),
        }
    }

    #[test]
    fn test_category_add_command() {
        let cli = parse_args(&["trt", "category", "add", "Work"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Add { name, description } => {
                    assert_eq!(name, "Work");
                    assert!(description.is_none());
                }
                _ => panic!("Expected Category Add command"),
            },
            _ => panic!("Expected Category command"),
        }

        let cli = parse_args(&[
            "trt",
            "category",
            "add",
            "Work",
            "--description",
            "Job stuff",
        ]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Add { name, description } => {
                    assert_eq!(name, "Work");
                    assert_eq!(description, Some("Job stuff".to_string()));
                }
                _ => panic!("Expected Category Add command"),
            },
            _ => panic!("Expected Category command"),
        }
    }

    #[test]
    fn test_category_update_command() {
        // Renaming alone still parses, `new_name` having become optional.
        let cli = parse_args(&["trt", "category", "update", "Home", "Personal"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Update {
                    old_name,
                    new_name,
                    description,
                    clear_description,
                } => {
                    assert_eq!(old_name, "Home");
                    assert_eq!(new_name, Some("Personal".to_string()));
                    assert!(description.is_none());
                    assert!(!clear_description);
                }
                _ => panic!("Expected Category Update command"),
            },
            _ => panic!("Expected Category command"),
        }

        // A description-only update, with no new name.
        let cli = parse_args(&[
            "trt",
            "category",
            "update",
            "Home",
            "--description",
            "Household chores",
        ]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Update {
                    new_name,
                    description,
                    ..
                } => {
                    assert!(new_name.is_none());
                    assert_eq!(description, Some("Household chores".to_string()));
                }
                _ => panic!("Expected Category Update command"),
            },
            _ => panic!("Expected Category command"),
        }

        // `--clear-description` needs no value.
        let cli = parse_args(&["trt", "category", "update", "Home", "--clear-description"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Update {
                    clear_description, ..
                } => assert!(clear_description),
                _ => panic!("Expected Category Update command"),
            },
            _ => panic!("Expected Category command"),
        }

        // `--description` and `--clear-description` contradict each other.
        assert!(try_parse_args(&[
            "trt",
            "category",
            "update",
            "Home",
            "--description",
            "x",
            "--clear-description",
        ])
        .is_err());
    }

    #[test]
    fn test_category_order_command() {
        let cli = parse_args(&["trt", "category", "order", "Work", "2"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Order { category, position } => {
                    assert_eq!(category, "Work");
                    assert_eq!(position, 2);
                }
                _ => panic!("Expected Category Order command"),
            },
            _ => panic!("Expected Category command"),
        }

        // Position must parse as a non-negative integer; clap rejects
        // anything else before it ever reaches `main`.
        assert!(try_parse_args(&["trt", "category", "order", "Work", "-1"]).is_err());
        assert!(try_parse_args(&["trt", "category", "order", "Work", "nope"]).is_err());
        assert!(try_parse_args(&["trt", "category", "order", "Work"]).is_err());
    }

    #[test]
    fn test_category_reorder_command() {
        let cli = parse_args(&["trt", "category", "reorder", "Work", "Home", "Personal"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Reorder { categories } => {
                    assert_eq!(
                        categories,
                        vec![
                            "Work".to_string(),
                            "Home".to_string(),
                            "Personal".to_string()
                        ]
                    );
                }
                _ => panic!("Expected Category Reorder command"),
            },
            _ => panic!("Expected Category command"),
        }

        // A single category is a legal (if degenerate) reorder.
        let cli = parse_args(&["trt", "category", "reorder", "Work"]);
        match cli.command {
            Commands::Category { command } => match command {
                CategoryCommands::Reorder { categories } => {
                    assert_eq!(categories, vec!["Work".to_string()]);
                }
                _ => panic!("Expected Category Reorder command"),
            },
            _ => panic!("Expected Category command"),
        }
    }

    #[test]
    fn test_category_reorder_requires_at_least_one_category() {
        // An empty reorder has nothing to do and is refused at parse time
        // rather than accepted as a silent no-op.
        assert!(try_parse_args(&["trt", "category", "reorder"]).is_err());
    }

    #[test]
    fn test_config_commands() {
        // Test config set
        let cli = parse_args(&["trt", "config", "set", "storage.type=json"]);
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
        let cli = parse_args(&["trt", "config", "list"]);
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
        let cli = parse_args(&["trt", "deleted", "list"]);
        match cli.command {
            Commands::Deleted { command } => match command {
                DeletedCommands::List => {}
                _ => panic!("Expected Deleted List command"),
            },
            _ => panic!("Expected Deleted command"),
        }

        // Test deleted restore
        let cli = parse_args(&["trt", "deleted", "restore", "Buy milk"]);
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
        let cli = parse_args(&["trt", "deleted", "flush"]);
        match cli.command {
            Commands::Deleted { command } => match command {
                DeletedCommands::Flush { yes } => assert!(!yes),
                _ => panic!("Expected Deleted Flush command"),
            },
            _ => panic!("Expected Deleted command"),
        }

        // ... in any of its three spellings.
        for flag in ["--yes", "-y", "--force"] {
            let cli = parse_args(&["trt", "deleted", "flush", flag]);
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
        assert!(try_parse_args(&["trt", "deleted", "restore"]).is_err());
    }

    #[test]
    fn test_required_arguments() {
        // `add` used to require `--category`; it is now optional so the
        // `category use` context and the `default-category` config value
        // can supply it. Parsing must therefore *succeed* without it - the
        // decision of where the task lands moved to `main`, which has the
        // storage and config access needed to make it.
        let result = try_parse_args(&["trt", "add", "Buy milk"]);
        assert!(result.is_ok());

        // A title is still required, though: nothing can supply that.
        let result = try_parse_args(&["trt", "add"]);
        assert!(result.is_err());

        // Test that priority must be valid
        let result = try_parse_args(&[
            "trt",
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
        let cli = parse_args(&["trt", "x", "Buy milk", "--category", "Home"]);
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
        let cli = parse_args(&["trt", "mark", "Buy milk", "--category", "Home"]);
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
        let cli = parse_args(&["trt", "o", "Buy milk", "--category", "Home"]);
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
        let cli = parse_args(&["trt", "unmark", "Buy milk", "--category", "Home"]);
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
        let cli = parse_args(&["trt", "move", "Buy milk", "--to", "Shopping"]);
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
            "trt", "move", "--from", "Home", "--to", "Shopping", "--task", "Buy milk",
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
        let cli = parse_args(&["trt", "move", "--from", "Home", "--task", "Buy milk"]);
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
