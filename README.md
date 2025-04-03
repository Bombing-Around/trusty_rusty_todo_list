# Trust Rusty TODO List

This is meant to be a _simple_ cli todo list application.

I intend to build incrementally on this application as I go.

For its first phase I would like to define a simple [interface](##Interface) for creating, updating, deleting, checking off/on tasks as well as a way to interact with categories, to which tasks will belong. It should also have a centralized configuraion storage and will, initially store to a JSON file, though ultimately I may want to implement SQLite storage.

A second phase may want to implementing scheduling using Dates and Times for Due Dates.

A third phase may try to allow usage with a scheduler / cron / etc. that would allow the system to periodically remind you of tasks that are due or overdo

Finally, I would love to implement some kind of syncing interface to keep your todos insync across your systems

## Build

`cargo build` 

## Interface 

The binary named `trtodo` will accept various arguments

| Command                                                                                               | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `trtodo add <title> --category <category_name or category_id> (or -c) [--priority <high|medium|low>]` | Add a new task with the given title and optional priority                                                                                 |
| `trtodo delete <title or id> --category <category_name or category_id> (or -c)`                       | Delete the task with the given title                                                                                                      |
| `trtodo update <title or id> --to <new_title> --category <category_name or category_id> (or -c)`      | Update the task with the given title                                                                                                      |
| `trtodo check (x, mark) <title or id> --category <category_name or category_id> (or -c)`              | Check off the task with the given title                                                                                                   |
| `trtodo uncheck (o, unmark) <title or id> --category <category_name or category_id> (or -c)`          | Uncheck the task with the given title                                                                                                     |
| `trtodo check all`                                                                                    | Check off all tasks in current category                                                                                                   |
| `trtodo uncheck all`                                                                                  | Uncheck all tasks in current category                                                                                                     |
| `trtodo move <task_name or id> --to <category_name or ID>`                                            | Move task to another category (when in category context)                                                                                  |
| `trtodo move --from <category_name or ID> --to <category_name or ID> --task <task_name or task_id>`   | Move task from one category to another - optionally omitting the `--to` argument will place the task at the parent level (uncategorized)  |
| `trtodo list [--search <term>] [--completed] [--priority <high|medium|low>]`                          | List all tasks with their IDs, optionally filtered                                                                                        |
| `trtodo category use <category_name or category_id>`                                                  | Use category for subsequent task interaction                                                                                              |
| `trtodo category clear`                                                                               | Clear the current category context                                                                                                        |
| `trtodo category show`                                                                                | Show current category context                                                                                                             |
| `trtodo category add <name>`                                                                          | Add a new category with the given name                                                                                                    |
| `trtodo category delete <name> (--new-category <category_name or category_id>)`                       | Delete a category and optionally move its tasks                                                                                           |
| `trtodo category update <old_name> <new_name>`                                                        | Update an existing category with the given name                                                                                           |
| `trtodo category list`                                                                                | List all categories with their IDs                                                                                                        |
| `trtodo config set <key=value>`                                                                       | Set configuration key to value                                                                                                            |
| `trtodo config default <key>`                                                                         | Unsets the value for key to force use of the default value                                                                                |
| `trtodo config list`                                                                                  | List all configuration keys and their values, including defaults which will be indicated with an asterisk                                 |
| `trtodo deleted flush`                                                                                | Remove all deleted items from "Deleted" category                                                                                          |
| `trtodo --help`                                                                                       | List these commands                                                                                                                       |
| `trtodo --help <command>`                                                                             | Describe command and its arguments                                                                                                        |
| `trtodo --config <path>`                                                                              | Uses a configuration file named `trtodo-config.json` in the referenced path                                                               |

## Additional Behaviors

The first time `trtodo` is run it should offer to create the default categories of "Home" and "Work" and create a configuration file under `.config\trtodo\` or `C:\\Users\\<username>\\AppData\\Roaming\trtodo`.

When operating on a `task_name`, the application will try to match the name - if it encounters the same name in multiple categories, it will prompt the user for which item on which to operate.

When deleting an item it will be _soft_deleted_ and placed under a hidden magic category "Deleted" with the category_id of 0. Items in this list are purged every _n_ days, a value that is configurable.

When deleting a category it is removed and its ID is made available again. All associated tasks are moved to the top unless a new category is provided.

Category context (set via `category use`) persists between runs of the application. When in a category context, commands that require category specification can omit the `--category` argument.

## Configuration Values

Configuration values are stored in `trtodo-config.json`. By default it's written to a config folder unless it's first read in your home directory. 

| Config Key              | Default Value      | Options             | Description                                                                                                                           |
| ----------------------- | ------------------ | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `deleted-task-lifespan` | `0`                | integer<1..?>       | Number of days before task in Deleted category are deleted. A value of 0, the default, indicates they are never automatically deleted |
| `storage.type`          | `json`             | `json\|sqlite`      | Type of storage backend to use                                                                                                        |
| `storage.path`          | `~/.config/trtodo` | string              | Path to storage location                                                                                                              |
| `default-category`      | `null`             | string              | Default category to use when no category is specified                                                                                 |
| `default-priority`      | `medium`           | `high\|medium\|low` | Default priority for new tasks                                                                                                        |

## Database Migrations

When using SQLite storage, the application includes a migration system to handle schema changes. This system:

1. Tracks the current schema version in a `schema_version` table
2. Automatically applies pending migrations on startup
3. Supports both forward migrations (up) and backward migrations (down)
4. Uses transactions to ensure data consistency
5. Provides rollback capabilities if needed

### Adding New Migrations

To add a new migration:

1. Add a new migration to the `MIGRATIONS` array in `src/storage/migrations.rs`:
```rust
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 2,
        up: "ALTER TABLE tasks ADD COLUMN due_date TEXT;",
        down: "ALTER TABLE tasks DROP COLUMN due_date;",
    },
];
```

2. The migration system will automatically:
   - Detect the current schema version
   - Apply any pending migrations in sequence
   - Handle rollbacks if needed
   - Maintain data integrity using transactions

### Migration Guidelines

1. Each migration should have a unique version number
2. Migrations should be idempotent (safe to run multiple times)
3. Down migrations should exactly reverse the changes made in the up migration
4. Use transactions to ensure data consistency
5. Test both up and down migrations thoroughly

### Example Migration

```rust
Migration {
    version: 2,
    up: r#"
        -- Add new column with default value
        ALTER TABLE tasks ADD COLUMN due_date TEXT;
        -- Update existing rows with a default value
        UPDATE tasks SET due_date = datetime('now') WHERE due_date IS NULL;
    "#,
    down: "ALTER TABLE tasks DROP COLUMN due_date;",
}
```

### Migration Process

1. When the application starts with SQLite storage:
   - Checks if the schema version table exists
   - Creates it if it doesn't exist
   - Sets initial version if needed
   - Applies any pending migrations

2. During migration:
   - Each migration runs in a transaction
   - If a migration fails, the transaction is rolled back
   - The schema version is updated only after successful migration
   - Migrations are applied in sequence by version number

3. Rollback process:
   - Migrations can be rolled back to any previous version
   - Down migrations are applied in reverse order
   - Each rollback runs in a transaction
   - Schema version is updated after each successful rollback