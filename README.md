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
| `trtodo add <title> [--category <category_name or category_id> (or -c)] [--priority <high|medium|low>]` | Add a new task with the given title and optional priority. Omitting `--category` uses the current category context, else `default-category`, else Uncategorized |
| `trtodo delete <title or id> [--category <category_name or category_id> (or -c)]`                     | Delete the task with the given title                                                                                                      |
| `trtodo update <title or id> --to <new_title> [--category <category_name or category_id> (or -c)]`    | Update the task with the given title                                                                                                      |
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
| `trtodo category order <category_name or category_id> <position>`                                     | Move a category to the given 1-based position in `category list`                                                                          |
| `trtodo category reorder <category_name or category_id>...`                                           | Set the order of several categories at once, in the order given                                                                           |
| `trtodo config set <key=value>`                                                                       | Set configuration key to value                                                                                                            |
| `trtodo config default <key>`                                                                         | Unsets the value for key to force use of the default value                                                                                |
| `trtodo config list`                                                                                  | List all configuration keys and their values, including defaults which will be indicated with an asterisk                                 |
| `trtodo deleted list`                                                                                 | List all soft-deleted tasks with their IDs, titles, original categories, and deletion dates - i.e. exactly what a `flush` would destroy   |
| `trtodo deleted restore <title or id>`                                                                | Restore a soft-deleted task to its original category. Matches soft-deleted tasks only, prompting if the title is ambiguous                |
| `trtodo deleted flush [--yes (or -y, --force)]`                                                       | Permanently remove all soft-deleted tasks, listing them and asking for confirmation first. `--yes` skips the prompt                       |
| `trtodo --help`                                                                                       | List these commands                                                                                                                       |
| `trtodo --help <command>`                                                                             | Describe command and its arguments                                                                                                        |
| `trtodo --config <path>`                                                                              | Uses the configuration file at the given path instead of the default. May also be set via the `TRTODO_CONFIG` environment variable; the flag wins  |
| `trtodo --yes` (or `-y`)                                                                              | Assume "yes" for confirmation prompts and never read from stdin                                                                           |
| `trtodo --no-input`                                                                                   | Never prompt: decline confirmations and never read from stdin                                                                             |

## Additional Behaviors

The first time `trtodo` is run it should offer to create the default categories of "Home" and "Work" and create a configuration file under `.config\trtodo\` or `C:\\Users\\<username>\\AppData\\Roaming\trtodo`.

That offer defaults to yes (a bare Enter accepts it) and is made at most once:

- It is made by the first command that touches task storage, not by `trtodo config ...`. Config commands never open task storage, and creating categories from a read-only `config list` - or from the very `config set storage.path=...` that decides where task data belongs - would put data somewhere the user did not ask for.
- The answer is recorded in the configuration file, so no later run asks again. Accepting, declining, and already having categories (an existing install, upgraded or otherwise) all count as answered; deleting every category afterwards is a legitimate empty state and does not bring the offer back.
- With no terminal attached (a pipe, a cron job, CI) there is nobody to ask, so the offer is silently skipped: nothing is created, nothing is recorded, and nothing blocks waiting for input. Pass `--yes` or `--no-input` to give a definite answer from a script.

When operating on a `task_name`, the application will try to match the name - if it encounters the same name in multiple categories, it will prompt the user for which item on which to operate.

When deleting an item it will be _soft_deleted_: it is marked with a deletion timestamp but keeps its real category, so restoring it is lossless. Soft-deleted items are hidden from listings and searches, and are purged automatically after `deleted-task-lifespan` days (0, the default, means never).

Because soft-deleted tasks are hidden everywhere else, the `deleted` namespace is where they can be seen and acted on. `deleted list` shows them, oldest deletion first - the order in which a flush or the automatic purge reaches them. `deleted restore` puts one back in the category it was deleted from; it resolves its argument among soft-deleted tasks only, so it can never match a live task, and it takes no `--category` (run `deleted list` to get the ID).

`deleted flush` is the only irreversible command in the application, so it lists what it is about to destroy and asks for confirmation before doing it. Confirmation is skipped with `--yes` (also spelled `-y` or `--force`). With no terminal attached - a pipe, a script, CI - there is nobody to ask, so a `flush` without `--yes` **refuses and exits non-zero** rather than destroying data unattended; scripts that mean it can say so with `--yes`. Flushing when nothing is soft-deleted has nothing at stake and so is a silent no-op that never prompts.

When deleting a category it is removed and its ID is made available again. All associated tasks are moved to the top unless a new category is provided.

Category context (set via `category use`) persists between runs of the application. When in a category context, commands that require category specification can omit the `--category` argument.

`category list` sorts by a custom order, not by ID or creation time - `category order` and `category reorder` are how that order is set. Positions are 1-based, matching every other user-facing identifier in this application (task and category IDs both start at 1). `Uncategorized` cannot be reordered: it is a synthesized category, not a stored one, and it is always sorted first regardless of what real categories do. `category order <category> <position>` sets that one category's position directly; two categories sharing a position is not an error, they simply tie and sort alphabetically by name relative to each other, and nothing else is displaced. `category reorder <category>...` sets several categories' positions at once, 1 through N in the order listed; categories left out of the list keep whatever position they already had, rather than being pushed after the ones just reordered - list every category to fully control the result.

`add` cannot fall back to searching every category the way `delete`/`update`/`check`/`uncheck` do - a new task has to land somewhere definite - so it resolves its category in strict order: an explicit `--category`, then the current category context, then the `default-category` config value, then Uncategorized. The category context deliberately outranks `default-category`: `category use` is a deliberate "I am working here right now", so `category use` temporarily overrides the configured default and `category clear` returns to it.

## Configuration Values

Configuration values are stored in `trtodo-config.json`. By default it's written to a config folder unless it's first read in your home directory. 

The keys below are the configurable ones - they are exactly what `config set`, `config default`, and `config list` operate on. The file may also contain internal bookkeeping that is not a setting (currently `default_categories_offered`, which records that the first-run offer above has been answered); those are not listed by `config list` and cannot be set with `config set`.

| Config Key              | Default Value      | Options             | Description                                                                                                                           |
| ----------------------- | ------------------ | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `deleted-task-lifespan` | `0`                | integer<1..?>       | Number of days before task in Deleted category are deleted. A value of 0, the default, indicates they are never automatically deleted |
| `storage.type`          | `json`             | `json\|sqlite`      | Type of storage backend to use                                                                                                        |
| `storage.path`          | `~/.config/trtodo` | string              | Path to storage location                                                                                                              |
| `default-category`      | `null`             | string              | Category `add` files new tasks into when no `--category` is given and no category context is set. Must name an existing category (by name or ID) - `add` reports an error rather than guessing if it no longer resolves. Renaming a category does not update this setting |
| `default-priority`      | `medium`           | `high\|medium\|low` | Default priority for new tasks                                                                                                        |

### Storage backends

`storage.path` is a *directory*. Each backend keeps its own data file inside it, so they can never be pointed at the same file and overwrite one another:

| `storage.type` | Data file          |
| -------------- | ------------------ |
| `json`         | `trtodo-data.json` |
| `sqlite`       | `trtodo-data.db`   |

Because of that, changing `storage.type` moves your tasks and categories to the new backend's file:

- If the backend you are leaving has data and the one you are switching to is empty, everything (including task and category IDs, and your `category use` context) is copied across. The old file is left exactly as it was, so switching back always gets you to it.
- If the backend you are switching to *already* has tasks or categories of its own, nothing is copied and nothing is overwritten. You are told what is in each store and how to switch back. The two are never merged, because merging would mean renumbering IDs.
- If there is nothing to move, the switch is silent.
