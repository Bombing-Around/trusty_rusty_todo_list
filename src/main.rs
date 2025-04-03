mod cli;
mod models;
mod storage;

use clap::Parser;
use cli::Cli;

fn main() {
    let _cli = Cli::parse();

    // TODO: Implement command handling
    println!("Command handling not yet implemented");
}
