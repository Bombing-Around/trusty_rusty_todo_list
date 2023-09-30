use clap::{App, Arg, SubCommand};

pub fn build_cli() -> App<'static, 'static> {
    App::new("Trust Rusty Todo List")
        .subcommand(SubCommand::with_name("add")
            .arg(Arg::with_name("title")
                .required(true)
                .index(1)))
        // Additional CRUD commands here
}
