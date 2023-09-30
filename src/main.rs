mod cli;
mod models;

use clap::Clap;
use models::{Todo, Folder};

fn main() {
    let matches = cli::build_cli().get_matches();

    if let Some(matches) = matches.subcommand_matches("add") {
        let title = matches.value_of("title").unwrap();
        // Handle add todo
    }
    // Handle other CRUD operations
}
