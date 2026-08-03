mod category_manager;
mod cli;
mod config;
mod models;
mod prompter;
mod storage;
mod task_manager;

use category_manager::{CategoryManager, UNCATEGORIZED_ID};
use clap::Parser;
use cli::{CategoryCommands, Cli, Commands, ConfigCommands, DeletedCommands};
use config::{ConfigError, ConfigManager};
use models::{Category, CategoryError, Priority, PriorityError, StorageError, Task};
use prompter::{NonInteractivePrompter, PromptError, Prompter, StdinPrompter};
use std::path::PathBuf;
use storage::{create_storage, migrate_storage, MigrationOutcome, Storage, StorageType};
use task_manager::{confirm_flush, TaskManager, TaskManagerError};
use thiserror::Error;

/// Top-level application error type.
///
/// Wraps errors from the various subsystems (config, storage, categories, and
/// tasks) so `main` has a single place to format and report failures.
#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Category(#[from] CategoryError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Task(#[from] TaskManagerError),
    #[error(transparent)]
    Priority(#[from] PriorityError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid key=value pair '{0}': expected format key=value")]
    MalformedKeyValue(String),
    #[error("unknown storage.type '{0}': expected one of json, sqlite")]
    UnknownStorageType(String),
    #[error("could not determine the home directory; set storage.path explicitly")]
    NoHomeDirectory,
    /// README: category context lets commands "omit the --category
    /// argument" - but only once one has actually been set via `category
    /// use`. `CheckAll`/`UncheckAll` and the simple `move` syntax have no
    /// `--category` argument at all, so with no context set there is no way
    /// to know which category they mean.
    ///
    /// `hint` carries a command-specific alternative, because the commands
    /// that hit this have different escape hatches - and notably none of
    /// them accepts `--category`, so suggesting it (as this message once
    /// did) is advice the user cannot act on.
    #[error("no category context is set; run 'category use <category>' first{hint}")]
    NoCategoryContext { hint: &'static str },
    /// `Move` accepts two distinct syntaxes (see the README) modelled as one
    /// `Commands::Move` variant with all-`Option` fields; any combination of
    /// arguments that isn't exactly one of those two syntaxes is rejected
    /// here rather than silently guessed at.
    #[error(
        "invalid 'move' arguments: use either '<task> --to <category>' (in category context) \
         or '--from <category> --task <task> [--to <category>]' (omitting --to moves the task \
         to Uncategorized)"
    )]
    InvalidMoveArguments,
    /// `deleted flush` is the only irreversible command in the CLI, so it
    /// asks first. When there is no terminal to ask at -
    /// `StdinPrompter` detects that and returns `PromptError::NotInteractive`
    /// rather than blocking - the choice is between destroying the data
    /// unattended and refusing. It refuses: a script that meant to flush can
    /// say so with `--yes`, whereas a script that flushed by accident has no
    /// way to say it didn't mean it.
    ///
    /// `PromptError::NotInteractive`'s own message is not reused here; it
    /// talks about picking between multiple matches and offers `--category`,
    /// neither of which applies to a flush.
    #[error(
        "'deleted flush' permanently destroys every soft-deleted task, and there is no \
         terminal attached to confirm at; re-run interactively, or pass --yes to confirm \
         non-interactively ('deleted list' shows what would be destroyed)"
    )]
    FlushNotConfirmed,
    /// `default-category` is persisted without ever being checked against the
    /// category list - `ConfigManager::set` has no access to task storage, and
    /// even a name that was valid when it was set can be deleted afterwards.
    /// So `add` can reach step 3 of `resolve_add_category` and find nothing
    /// there.
    ///
    /// This is an error rather than a warning-and-fall-through to
    /// Uncategorized: the user named a destination for their new tasks, and
    /// quietly filing one somewhere else is the single outcome they are least
    /// likely to notice. It is also the consistent choice - an explicit
    /// `--category NoSuchThing` is already a hard error via
    /// `resolve_category`, and `default-category` is the same act of naming a
    /// category, just written down in advance. Erroring costs the user one
    /// re-run; a misfiled task costs them finding it again.
    #[error(
        "config 'default-category' names a category that does not exist: '{0}'; \
         create it, point 'default-category' at an existing category, or pass \
         --category explicitly"
    )]
    UnresolvableDefaultCategory(String),
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

    // One prompter for the whole invocation, chosen from the non-interactive
    // flags. Built here rather than inside each command so `--yes` /
    // `--no-input` mean the same thing everywhere.
    let mut prompter = build_prompter(&cli);

    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Set { key_value } => {
                let (key, value) = key_value
                    .split_once('=')
                    .ok_or_else(|| CliError::MalformedKeyValue(key_value.clone()))?;
                // Deliberately *before* the config is written: if carrying the
                // user's data across fails, the setting stays where it was and
                // they are still looking at their tasks.
                if key == "storage.type" {
                    carry_data_across_backend_switch(&config_manager, value)?;
                }
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
            let storage = open_task_storage(&mut config_manager, prompter.as_mut())?;
            run_category_command(&*storage, command)?;
        }
        Commands::Deleted { command } => {
            let storage = open_task_storage(&mut config_manager, prompter.as_mut())?;
            run_deleted_command(&*storage, command)?;
        }
        other => {
            let storage = open_task_storage(&mut config_manager, prompter.as_mut())?;
            run_task_command(&*storage, &config_manager, prompter.as_mut(), other)?;
        }
    }

    Ok(())
}

/// Picks the prompter for this invocation.
///
/// With neither flag the real `StdinPrompter` is used - which is still safe
/// for automation, because it detects a non-interactive stdin itself and
/// reports `PromptError::NotInteractive` rather than blocking on a
/// `read_line` nobody will answer.
fn build_prompter(cli: &Cli) -> Box<dyn Prompter> {
    if cli.yes {
        Box::new(NonInteractivePrompter::assuming_yes())
    } else if cli.no_input {
        Box::new(NonInteractivePrompter::assuming_no())
    } else {
        Box::new(StdinPrompter)
    }
}

/// The categories a first run offers to create, in the order they get
/// created (README: "offer to create the default categories of 'Home' and
/// 'Work'").
const DEFAULT_CATEGORIES: [&str; 2] = ["Home", "Work"];

/// Opens task storage for a command that needs it, making the first-run
/// offer on the way.
///
/// This is the *only* path to task storage from `run`, which is what decides
/// the answer to "which commands trigger the offer?": every command that
/// touches tasks or categories does, and `trtodo config ...` - the one
/// command group that never opens task storage - does not. That is
/// deliberate:
///
///   - creating categories means writing the data file, so offering during
///     `config list` would turn a read-only query into a storage mutation
///     (and into a `create_dir_all`, see `open_storage`);
///   - `config set storage.path=...` is exactly how a user says where their
///     data should live, and prompting *before* that took effect would
///     create the default categories in the location they were in the middle
///     of changing;
///   - `--help` / `--version` never reach `run` at all, since clap exits
///     first, so they can't prompt either way.
///
/// The trade-off is that a user whose very first command is `config list`
/// gets the offer on their next real command instead - which is the right
/// time for it anyway.
fn open_task_storage(
    config_manager: &mut ConfigManager,
    prompter: &mut dyn Prompter,
) -> Result<Box<dyn Storage>, CliError> {
    let storage = open_storage(config_manager)?;
    offer_default_categories(&*storage, config_manager, prompter)?;
    Ok(storage)
}

/// Offers to create the README's default "Home" and "Work" categories the
/// first time `trtodo` touches task storage.
///
/// "First run" is a *config* fact, not a storage fact: the marker written by
/// `ConfigManager::record_default_categories_offer` records that the offer
/// has been resolved. Deciding from storage alone ("are there no categories?")
/// would re-offer forever to anyone who deliberately deleted every category,
/// and an earlier attempt at this feature was rejected for a related pair of
/// mistakes - creating the categories outright rather than offering, and
/// doing it on every invocation rather than once.
///
/// The offer is skipped, and the marker recorded silently, when categories
/// already exist: that is an established install (or one upgrading from a
/// version predating this feature), not a fresh one, and it must not be
/// interrupted to be asked about categories it clearly doesn't need.
///
/// A `PromptError` is *not* an answer, so it records nothing and prints
/// nothing: with no terminal attached (a pipe, a cron job, CI) there is
/// nobody to ask, and the run must neither block nor quietly burn the
/// one-and-only offer that a human would have wanted to see. Automation that
/// wants a definite answer passes `--yes` or `--no-input`.
fn offer_default_categories(
    storage: &dyn Storage,
    config_manager: &mut ConfigManager,
    prompter: &mut dyn Prompter,
) -> Result<(), CliError> {
    if config_manager.default_categories_offered() {
        return Ok(());
    }

    if !storage.get_all_categories()?.is_empty() {
        config_manager.record_default_categories_offer()?;
        return Ok(());
    }

    let question = format!(
        "It looks like this is your first run. Create the default categories {}?",
        DEFAULT_CATEGORIES.join(" and ")
    );
    let accepted = match prompter.confirm(&question, true) {
        Ok(accepted) => accepted,
        Err(_) => return Ok(()),
    };

    if accepted {
        let mut category_manager = CategoryManager::new(storage);
        for name in DEFAULT_CATEGORIES {
            let id = category_manager.add_category(name.to_string(), None)?;
            println!("Category '{}' added with ID {}", name, id);
        }
    } else {
        println!("Skipped; add categories yourself with 'trtodo category add <name>'");
    }

    config_manager.record_default_categories_offer()?;
    Ok(())
}

/// Builds the task storage backend described by the current configuration.
///
/// `storage.path` is the storage *location* (a directory, per the README's
/// `~/.config/trtodo` default); the data file inside it is named after the
/// chosen backend.
///
/// Also sweeps up anything overdue for automatic purging under
/// `deleted-task-lifespan` before handing the storage back. This
/// is the one place every command that touches task storage passes through -
/// `Commands::Config` never calls `open_storage` at all, since it has no
/// need to touch task storage, so the sweep correctly never runs for it (and
/// never pays `open_storage`'s side effect of creating directories, either).
/// Piggybacking here means a future new command can't forget to wire the
/// sweep in, and it stays cheap and silent on the common path: the
/// documented default threshold is 0 ("never"), and `purge_deleted_tasks`
/// itself short-circuits before touching disk in that case.
fn open_storage(config_manager: &ConfigManager) -> Result<Box<dyn Storage>, CliError> {
    let storage_type = configured_storage_type(config_manager)?;
    let storage = open_storage_of_type(config_manager, storage_type)?;
    purge_expired_deleted_tasks(&*storage, config_manager)?;

    Ok(storage)
}

/// The backend named by `storage.type`. `ConfigManager::get` already resolves
/// the documented default (`json`), so `None` here only happens if that
/// default ever goes missing - treated the same as any other unrecognised
/// value rather than silently assumed.
fn configured_storage_type(config_manager: &ConfigManager) -> Result<StorageType, CliError> {
    let value = config_manager
        .get("storage.type")
        .unwrap_or_else(|| "json".to_string());
    StorageType::parse(&value).ok_or(CliError::UnknownStorageType(value))
}

/// The `storage.path` *directory* every backend keeps its data file in. See
/// `StorageType::data_file_name` for why the file name varies by backend.
fn storage_dir(config_manager: &ConfigManager) -> Result<PathBuf, CliError> {
    match config_manager.get("storage.path") {
        Some(path) => Ok(PathBuf::from(shellexpand::tilde(&path).as_ref())),
        None => Ok(dirs::home_dir()
            .ok_or(CliError::NoHomeDirectory)?
            .join(".config")
            .join("trtodo")),
    }
}

/// Opens a *specific* backend against the configured storage directory,
/// without the automatic purge sweep.
///
/// `open_storage` is the entry point for commands, which want the configured
/// backend and want the sweep to run. This one exists for
/// `carry_data_across_backend_switch`, which needs to hold both the outgoing
/// and incoming backends open at once and must not have a purge fire against
/// a store it is merely reading in passing.
fn open_storage_of_type(
    config_manager: &ConfigManager,
    storage_type: StorageType,
) -> Result<Box<dyn Storage>, CliError> {
    let dir = storage_dir(config_manager)?;

    // SQLite cannot create the containing directory for us, and JSON only does
    // so on save - make sure it exists before either backend opens the file.
    std::fs::create_dir_all(&dir)?;

    Ok(create_storage(
        storage_type,
        &dir.join(storage_type.data_file_name()),
    )?)
}

/// Carries the user's tasks and categories over when `storage.type` changes,
/// and reports honestly when it can't.
///
/// Each backend keeps its own data file, so before this existed a switch made
/// every category and task disappear from view with nothing but "Warning:
/// Changing storage type may require data migration" - naming a migration
/// that did not exist - to explain it. Nothing was actually destroyed (the old
/// file sat there untouched, and switching back revealed it again), but it
/// read as data loss, which for a todo list is nearly as bad.
///
/// The copy itself is `storage::migrate_storage`, which owns the safety
/// rules: source is never written to, a non-empty destination is never
/// overwritten, IDs are never renumbered. All this function does is resolve
/// the two backends and turn the outcome into something a person can read.
///
/// Anything that isn't a real backend change is a silent no-op: setting
/// `storage.type` to what it already is, or to a value
/// `ConfigManager::set` is about to reject anyway.
fn carry_data_across_backend_switch(
    config_manager: &ConfigManager,
    new_value: &str,
) -> Result<(), CliError> {
    let Some(new_type) = StorageType::parse(new_value) else {
        // Not a backend we know. Say nothing and let `ConfigManager::set`
        // produce its own "must be one of: json, sqlite" error.
        return Ok(());
    };

    let old_type = configured_storage_type(config_manager)?;
    if old_type == new_type {
        return Ok(());
    }

    let old_file = storage_dir(config_manager)?.join(old_type.data_file_name());
    if !old_file.exists() {
        // Nothing was ever written under the outgoing backend - a first-ever
        // switch. Don't open the new backend just to tell the user nothing
        // happened.
        return Ok(());
    }

    // A source we can't read is the one failure that must NOT block the
    // switch. If the outgoing store is corrupt, unreadable, or written by a
    // build we don't understand, then the user can't see that data under the
    // old backend either - refusing to change `storage.type` would trap them
    // on the broken backend, and do it while reporting an error about the
    // backend they were trying to leave. Say what happened, leave the file
    // alone, and let the switch proceed.
    //
    // Failures on the *destination* side are handled the opposite way: they
    // propagate, so the setting stays put and the user keeps looking at the
    // store that still works.
    let source = match open_storage_of_type(config_manager, old_type)
        .and_then(|source| source.load().map(|_| source).map_err(CliError::from))
    {
        Ok(source) => source,
        Err(e) => {
            eprintln!(
                "Warning: could not read the existing {} store at {}, so nothing was migrated: {}",
                old_type.as_str(),
                old_file.display(),
                e
            );
            eprintln!(
                "Warning: the file has been left untouched. Run 'trtodo config set \
                 storage.type={}' to go back to it.",
                old_type.as_str()
            );
            return Ok(());
        }
    };
    let destination = open_storage_of_type(config_manager, new_type)?;

    match migrate_storage(&*source, &*destination)? {
        MigrationOutcome::SourceEmpty => {}
        MigrationOutcome::Migrated { tasks, categories } => {
            println!(
                "Migrated {} task(s) and {} categor{} from {} to {} storage.",
                tasks,
                categories,
                if categories == 1 { "y" } else { "ies" },
                old_type.as_str(),
                new_type.as_str()
            );
            println!(
                "Your previous {} data was left untouched at {}.",
                old_type.as_str(),
                old_file.display()
            );
        }
        MigrationOutcome::DestinationNotEmpty { tasks, categories } => {
            // Both stores hold real data. Refusing to merge them is the
            // deliberate choice (see `migrate_storage`); what matters is that
            // the user is told which data they are about to be looking at and
            // how to get back to the other set.
            eprintln!(
                "Warning: {} storage already holds {} task(s) and {} categor{}, so nothing was \
                 migrated and nothing was overwritten.",
                new_type.as_str(),
                tasks,
                categories,
                if categories == 1 { "y" } else { "ies" }
            );
            eprintln!(
                "Warning: your {} data is still intact at {}. Run 'trtodo config set \
                 storage.type={}' to go back to it.",
                old_type.as_str(),
                old_file.display(),
                old_type.as_str()
            );
        }
    }

    Ok(())
}

/// Runs the automatic, `deleted-task-lifespan`-gated purge (README: "purged
/// automatically after `deleted-task-lifespan` days") once per
/// invocation, from within `open_storage`. See that function's doc comment
/// for why it lives there rather than being duplicated across `run`'s
/// branches.
///
/// A stored `deleted-task-lifespan` that fails to parse as a plain integer
/// degrades to "don't purge" rather than surfacing an error: whatever the
/// user actually asked for (e.g. `list`) should not fail just because
/// cleanup housekeeping couldn't run.
fn purge_expired_deleted_tasks(
    storage: &dyn Storage,
    config_manager: &ConfigManager,
) -> Result<(), CliError> {
    let threshold = config_manager
        .get("deleted-task-lifespan")
        .and_then(|v| v.parse::<u32>().ok());

    if let Some(threshold) = threshold {
        TaskManager::new(storage).purge_expired_deleted(threshold)?;
    }

    Ok(())
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

/// Dispatches `trtodo deleted ...`. Mirrors `run_category_command`'s shape.
///
/// Unlike that function this also needs a `CategoryManager`: soft-deleted
/// tasks keep their real `category_id`, and both `list` and
/// `restore` are only useful if that is reported as a *name* the user
/// recognizes rather than a bare number.
fn run_deleted_command(storage: &dyn Storage, command: DeletedCommands) -> Result<(), CliError> {
    let category_manager = CategoryManager::new(storage);
    let task_manager = TaskManager::new(storage);
    // See `run_task_command` for why this is the `Prompter` seam and not an
    // inline `stdin().read_line()`.
    let mut prompter = StdinPrompter;

    match command {
        DeletedCommands::List => {
            let tasks = task_manager.list_deleted()?;
            println!("Deleted tasks:");
            // `list` prints its header and then nothing when there is
            // nothing to show, which is fine for a command whose answer is
            // "you have no tasks". This command's whole job is to answer
            // "what would a flush destroy?", and silence is a bad way to say
            // "nothing" - it is indistinguishable from a command that failed
            // to look.
            if tasks.is_empty() {
                println!("  (none)");
            }
            for task in &tasks {
                println!("{}", format_deleted_task(&category_manager, task)?);
            }
        }
        DeletedCommands::Restore { title_or_id } => {
            let task = task_manager.resolve_deleted_task(&title_or_id, &mut prompter)?;
            let title = task.title.clone();
            let original_category_id = task.category_id;
            let category_id = task_manager.restore_task(task)?;
            let category_name = category_display_name(&category_manager, category_id)?;
            if category_id == original_category_id {
                println!("Task '{}' restored to category '{}'", title, category_name);
            } else {
                // Only reachable from externally-edited data - see
                // `task_manager::restore_destination` - but if it ever
                // happens the user should hear that the task did not go back
                // where it came from.
                println!(
                    "Task '{}' restored to category '{}'; its original category no longer exists",
                    title, category_name
                );
            }
        }
        DeletedCommands::Flush { yes } => {
            let pending = task_manager.list_deleted()?;

            // Nothing at stake means nothing to confirm: a flush with an
            // empty deleted set is a documented no-op, and prompting for
            // permission to destroy zero tasks (or failing a script over it)
            // would be noise.
            if pending.is_empty() {
                println!("Flushed 0 deleted task(s)");
                return Ok(());
            }

            // Printed before the flush in both paths, so it serves as the
            // confirmation preview *and* as the record of what was destroyed
            // - one listing satisfies both, and printing it afterwards
            // instead would mean the interactive user confirms blind.
            println!(
                "The following {} task(s) will be permanently removed:",
                pending.len()
            );
            for task in &pending {
                println!("{}", format_deleted_task(&category_manager, task)?);
            }

            if !yes {
                match confirm_flush(&mut prompter, pending.len()) {
                    Ok(true) => {}
                    Ok(false) => {
                        println!("Flush cancelled; nothing was removed");
                        return Ok(());
                    }
                    Err(PromptError::NotInteractive) => return Err(CliError::FlushNotConfirmed),
                    Err(e) => return Err(TaskManagerError::from(e).into()),
                }
            }

            let flushed = task_manager.flush_deleted()?;
            println!("Flushed {} deleted task(s)", flushed.len());
        }
    }

    Ok(())
}

/// One row of `deleted list`: ID, title, the task's *real* category name,
/// and when it was deleted.
///
/// Shared with `flush`'s preview so the two commands describe the same task
/// in exactly the same words. Formatted after `run_task_command`'s `list`
/// (`<id>: <title> (...)`), minus the `[x]` completion marker: whether a
/// task about to be destroyed was ticked off first is not information
/// anybody is deciding on.
fn format_deleted_task(
    category_manager: &CategoryManager,
    task: &Task,
) -> Result<String, CliError> {
    let category_name = category_display_name(category_manager, task.category_id)?;
    Ok(format!(
        "{}: {} (category: {}, deleted: {})",
        task.id,
        task.title,
        category_name,
        task.deleted_at_display()
    ))
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

/// Converts a CLI-facing priority into the domain model's `Priority`.
/// Deliberately explicit rather than `#[derive]`d or `From`-shared: `cli::Priority`
/// exists purely to give clap a `ValueEnum`, and keeping the conversion here (in
/// the thin dispatch layer) means neither `cli` nor `models` needs to depend on
/// the other.
fn to_model_priority(priority: cli::Priority) -> Priority {
    match priority {
        cli::Priority::High => Priority::High,
        cli::Priority::Medium => Priority::Medium,
        cli::Priority::Low => Priority::Low,
    }
}

/// Resolves the effective `default-priority` config value to a domain
/// `Priority`, for `add` invocations that omit `--priority`. On a fresh
/// install this resolves to the documented default, `medium`, rather than
/// to nothing.
fn resolve_default_priority(config_manager: &ConfigManager) -> Result<Priority, CliError> {
    let value = config_manager
        .get("default-priority")
        .unwrap_or_else(|| "medium".to_string());
    Ok(Priority::from_str(&value)?)
}

/// Resolves the category a task created by `add` should land in.
///
/// `add` is the odd one out among the task commands: `Delete`, `Update`,
/// `Check`, and `Uncheck` can all degrade to an unscoped search (see
/// `resolve_task_scope`) because they have an existing task to find, but a
/// *new* task has to be written somewhere definite. So instead of degrading,
/// `add` walks a strict precedence chain, most specific first:
///
/// 1. an explicit `--category`,
/// 2. the `category use` context, if one has actually been set,
/// 3. the `default-category` config value,
/// 4. Uncategorized.
///
/// Step 2 outranking step 3 is deliberate. `category use` is a transient,
/// just-made statement of "I am working in here right now"; `default-category`
/// is persistent background configuration set once and then forgotten. Letting
/// the config win would make the README's promise that "when in a category
/// context, commands that require category specification can omit the
/// `--category` argument" quietly untrue for `add` alone, and would leave the
/// user no way to work outside their default without typing `--category` every
/// time. In the chosen order both settings stay reachable: `category use` to
/// override the default for a while, `category clear` to fall back to it
/// again.
///
/// Note step 2 checks `has_explicit_category_context` rather than reading
/// `get_current_category` directly, exactly as `resolve_task_scope` and
/// `require_category_context` do. `get_current_category` answers
/// `Some(UNCATEGORIZED_ID)` even when no context was ever set, so using it
/// here would collapse step 2 into step 4 and make steps 3 and 4 dead code -
/// `default-category` would go on being ignored, which is the entire bug this
/// resolves.
///
/// Returns an ID rather than a `Category` so a context pointing at a category
/// that has since vanished prints the same way `list` does (see
/// `category_display_name`) instead of needing a second failure mode here.
fn resolve_add_category(
    category_manager: &CategoryManager,
    config_manager: &ConfigManager,
    category: Option<String>,
) -> Result<u64, CliError> {
    // 1. An explicit `--category` always wins, and an unresolvable one is an
    //    error - unchanged behaviour from when this argument was mandatory.
    if let Some(reference) = category {
        return Ok(resolve_category(category_manager, &reference)?.id);
    }

    // 2. The `category use` context, but only if one was really set.
    if category_manager.has_explicit_category_context() {
        return Ok(category_manager
            .get_current_category()
            .expect("has_explicit_category_context() true implies Some(..)"));
    }

    // 3. The configured `default-category`. It stores a *name* (or ID) rather
    //    than a resolved ID on purpose: category IDs are handed back out after
    //    a delete, so a stored ID could silently start pointing at
    //    an unrelated category, whereas a stored name can only ever fail to
    //    resolve - a failure we can report. `ConfigManager::get` returns the
    //    effective value, and `default-category` has no documented default, so
    //    `None` here genuinely means "the user never set one".
    if let Some(reference) = config_manager.get("default-category") {
        return resolve_category(category_manager, &reference)
            .map(|category| category.id)
            .map_err(|_| CliError::UnresolvableDefaultCategory(reference));
    }

    // 4. Nothing said otherwise, so the task is simply uncategorized.
    Ok(UNCATEGORIZED_ID)
}

/// Resolves the scope `Delete`, `Update`, `Check`, and `Uncheck` should
/// search within, per the README: the explicit `--category` if given,
/// otherwise the current category context if one has been set via `category
/// use`, otherwise unscoped (`None`) - meaning search every category and let
/// `TaskManager::resolve_task`'s "same name in multiple categories, prompt
/// the user" rule fire if the title turns out to be ambiguous.
///
/// Unlike `require_category_context`, an unset context here is *not* an
/// error: these four commands all take a task reference to disambiguate
/// with (unlike `CheckAll`/`UncheckAll`/simple `move`, which have nothing to
/// disambiguate and so still require an explicit context via that
/// function).
fn resolve_task_scope(
    category_manager: &CategoryManager,
    category: Option<String>,
) -> Result<Option<u64>, CliError> {
    match category {
        Some(name) => Ok(Some(resolve_category(category_manager, &name)?.id)),
        None if category_manager.has_explicit_category_context() => {
            Ok(category_manager.get_current_category())
        }
        None => Ok(None),
    }
}

/// The category ID commands with *no* `--category` argument at all
/// (`CheckAll`/`UncheckAll`, the simple `move` syntax) must operate in.
///
/// Deliberately does NOT just call `CategoryManager::get_current_category`
/// directly: that method returns `Some(UNCATEGORIZED_ID)` even when nothing
/// was ever set (so `category show` has something sensible to print), and
/// treating that as "the user wants Uncategorized" here would silently
/// operate on the wrong tasks whenever the user simply forgot to set a
/// context, instead of telling them to set one.
/// `hint` is appended to the error when no context is set, so each caller can
/// offer the escape hatch that actually applies to it.
fn require_category_context(
    category_manager: &CategoryManager,
    hint: &'static str,
) -> Result<u64, CliError> {
    if category_manager.has_explicit_category_context() {
        Ok(category_manager
            .get_current_category()
            .expect("has_explicit_category_context() true implies Some(..)"))
    } else {
        Err(CliError::NoCategoryContext { hint })
    }
}

/// `move`'s simple syntax needs a context, but its extended syntax names both
/// categories outright and so needs none - worth pointing at.
const MOVE_CONTEXT_HINT: &str =
    ", or name the source explicitly with 'move --from <category> --task <task> --to <category>'";

/// Resolves a category ID to a display name for output, special-casing the
/// synthesized Uncategorized category the same way `CategoryCommands::Show`
/// does.
fn category_display_name(
    category_manager: &CategoryManager,
    category_id: u64,
) -> Result<String, CliError> {
    if category_id == UNCATEGORIZED_ID {
        return Ok("Uncategorized".to_string());
    }
    Ok(category_manager
        .get_category(category_id)?
        .map(|c| c.name)
        .unwrap_or_else(|| format!("(unknown category {})", category_id)))
}

/// Dispatches every task-related command (everything in `Commands` except
/// `Category`, `Config`, and `Deleted`, which `run` handles itself). Mirrors
/// `run_category_command`'s shape: build the managers once, match on the
/// command, print a short human-readable confirmation for each.
fn run_task_command(
    storage: &dyn Storage,
    config_manager: &ConfigManager,
    // The invocation's prompter (see `build_prompter`). Its `choose` detects
    // a non-interactive stdin (e.g. this binary run from a test harness or a
    // script) and returns a clean `PromptError::NotInteractive` instead of
    // blocking - see `crate::prompter` for why this is a trait rather than an
    // inline `stdin().read_line()` call.
    prompter: &mut dyn Prompter,
    command: Commands,
) -> Result<(), CliError> {
    let category_manager = CategoryManager::new(storage);
    let task_manager = TaskManager::new(storage);

    match command {
        Commands::Add {
            title,
            category,
            priority,
        } => {
            let category_id = resolve_add_category(&category_manager, config_manager, category)?;
            let priority = match priority {
                Some(p) => to_model_priority(p),
                None => resolve_default_priority(config_manager)?,
            };
            let id = task_manager.add_task(title.clone(), category_id, priority, None)?;
            // Always name the category that was actually used, even when the
            // user did not: with `--category` now optional, the confirmation
            // line is how they find out which of the four resolution steps
            // won.
            let category_name = category_display_name(&category_manager, category_id)?;
            println!(
                "Task '{}' added with ID {} in category '{}'",
                title, id, category_name
            );
        }
        Commands::Delete {
            title_or_id,
            category,
        } => {
            let scope = resolve_task_scope(&category_manager, category)?;
            let task = task_manager.resolve_task(&title_or_id, scope, prompter)?;
            task_manager.delete_task(task.id)?;
            println!("Task '{}' deleted", task.title);
        }
        Commands::Update {
            title_or_id,
            new_title,
            category,
        } => {
            let scope = resolve_task_scope(&category_manager, category)?;
            let task = task_manager.resolve_task(&title_or_id, scope, prompter)?;
            let old_title = task.title.clone();
            task_manager.rename_task(task, new_title.clone())?;
            println!("Task '{}' renamed to '{}'", old_title, new_title);
        }
        Commands::Check {
            title_or_id,
            category,
        } => {
            let scope = resolve_task_scope(&category_manager, category)?;
            let task = task_manager.resolve_task(&title_or_id, scope, prompter)?;
            let title = task.title.clone();
            task_manager.set_completed(task, true)?;
            println!("Task '{}' checked off", title);
        }
        Commands::Uncheck {
            title_or_id,
            category,
        } => {
            let scope = resolve_task_scope(&category_manager, category)?;
            let task = task_manager.resolve_task(&title_or_id, scope, prompter)?;
            let title = task.title.clone();
            task_manager.set_completed(task, false)?;
            println!("Task '{}' unchecked", title);
        }
        Commands::CheckAll => {
            let category_id = require_category_context(&category_manager, "")?;
            let count = task_manager.set_all_completed(category_id, true)?;
            println!("Checked off {} task(s)", count);
        }
        Commands::UncheckAll => {
            let category_id = require_category_context(&category_manager, "")?;
            let count = task_manager.set_all_completed(category_id, false)?;
            println!("Unchecked {} task(s)", count);
        }
        Commands::Move {
            task_name_or_id,
            to_category,
            from_category,
            task,
        } => {
            // `Move` is one variant covering two distinct syntaxes (README):
            //   simple:   `<task> --to <category>`             (needs a category context)
            //   extended: `--from <category> --task <task> [--to <category>]`
            // Any other combination of the four optional fields is rejected
            // outright rather than guessed at.
            let (task_ref, scope_category_id, target_category_id) =
                match (task_name_or_id, to_category, from_category, task) {
                    (Some(task_ref), Some(to), None, None) => {
                        let scope = require_category_context(&category_manager, MOVE_CONTEXT_HINT)?;
                        let target = resolve_category(&category_manager, &to)?.id;
                        (task_ref, scope, target)
                    }
                    (None, to, Some(from), Some(task_ref)) => {
                        let scope = resolve_category(&category_manager, &from)?.id;
                        // Omitting --to in the extended syntax means "move to
                        // Uncategorized" (README).
                        let target = match to {
                            Some(name) => resolve_category(&category_manager, &name)?.id,
                            None => UNCATEGORIZED_ID,
                        };
                        (task_ref, scope, target)
                    }
                    _ => return Err(CliError::InvalidMoveArguments),
                };

            let task = task_manager.resolve_task(&task_ref, Some(scope_category_id), prompter)?;
            task_manager.move_task(task.id, target_category_id)?;
            let target_name = category_display_name(&category_manager, target_category_id)?;
            println!("Task '{}' moved to '{}'", task.title, target_name);
        }
        Commands::List {
            search,
            completed,
            priority,
        } => {
            let priority = priority.map(to_model_priority);
            let tasks = task_manager.list_tasks(search.as_deref(), completed, priority)?;
            println!("Tasks:");
            for task in tasks {
                let status = if task.completed { "x" } else { " " };
                let category_name = category_display_name(&category_manager, task.category_id)?;
                println!(
                    "{}: [{}] {} (priority: {}, category: {})",
                    task.id,
                    status,
                    task.title,
                    task.priority.to_str(),
                    category_name
                );
            }
        }
        Commands::Category { .. } | Commands::Config { .. } | Commands::Deleted { .. } => {
            unreachable!("Category/Config/Deleted are dispatched in `run` before reaching here")
        }
    }

    Ok(())
}
