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

The binary name `trtodo` will accept various arguments

| Command | Description |
| ------- | ----------- |
| `trtodo add <title> --category <category_name or category_id> (or -c)` | Add a new task with the given title |
| `trtodo delete <title or id> --category <category_name or category_id> (or -c)` | Delete the task with the given title |
| `trtodo update <title or id> --to <new_title> --category <category_name or category_id> (or -c)` | Update the task with the given title |
| `trtodo check (x, mark) <title or id> --category <category_name or category_id> (or -c)` | Check off the task with the given title |
| `trtodo uncheck (o, unmark) <title or id> --category <category_name or category_id> (or -c)` | Uncheck the task with the given title |
| `trtodo move --from <category_name or ID> --to <category_name or ID> --task <task_name or task_id>` | Move task from one category to another - optionally omitting the `--to` argument will place the task at the parent level (uncategorized) |
| `trtodo list` | List all tasks with their IDs |
| `trtodo category use <category_name or category_id>` | Use category for subsequent task interaction |
| `trtodo category add <name>` | Add a new category with the given name |
| `trtodo category deleted <name> (--new-category <category_name  or category_id>)` | Add a new category with the given name |
| `trtodo category update <old_name> <new_name>` | Update an existing category with the given name |
| `trtodo category list` | List all categories with their IDs |
| `trtodo config set <key=value>` | Set configuration key to value |
| `trtodo config default <key>` | Unsets the value for key to force use of the default value |
| `trtodo config list` | List all configuraion keys and their values, including defaults which will be indicated with an asterisk |
| `trtodo flushdeleteditems` | Remove all deleted items from "Deleted" category |
| `trtodo --help` | List these commands
| `trtodo --help <command>` | Describe command and its arguments
| `trtodo --config <path>` | Uses a configuration file named `trtodo-config.json` in the referenced path |

## Additional Behaviors

The first time `trtodo` is run it should offer to create the default categories of "Home" and "Work" and create a configuraton file under `.config\trtodo\` or `C:\\Users\\<username>\\AppData\\Roaming\trtodo`.

When operating on a `task_name`, the application will try to match the name - if it encounters the same name in multiple categories, it will prompt the user for which item on which to operate.

When deleting an item it will be _soft_deleted_ and placed under a hidden magic category "Deleted" with the category_id of 0. Items in this list are purged every _n_ days, a value that is configurable.

When deleting a category it is removed and its ID is made available again. All associated tasks are moved to the top unless a new category is provided.

## Configuration Values

Configuration values are stored in `trtodo-config.json`. By default it's written to a config folder unless it's first read in your home directory. 

| Config Key | Default Value | Options | Description |
| ---------- | ------------- | ------- | ----------- |
| `deleted-task-lifespan` | `0` | integer<1..?> | Number of days before task in Deleted category are deleted. A value of 0, the default, indicates they are never automatically deleted |