mod cli;
mod models;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    // TODO: Implement command handling
    match cli.command {
        _ => println!("Command handling not yet implemented"),
    }
}
