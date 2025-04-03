mod cli;
mod models;

use cli::Cli;
use clap::Parser;

fn main() {
    let cli = Cli::parse();
    
    // TODO: Implement command handling
    match cli.command {
        _ => println!("Command handling not yet implemented"),
    }
}
