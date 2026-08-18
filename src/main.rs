use clap::{ArgAction, Command, arg, error::ErrorKind};
use std::ffi::OsString;
use std::path::PathBuf;

mod model;

fn cli() -> Command {
    Command::new("rad")
        .about("A music management tool")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            arg!(-v --verbose "Increase output verbosity.")
                .action(ArgAction::Count)
                .global(true),
        )
        .arg(
            arg!(-t --test "Dry run subcommand. Don't make any changes to the database.")
                .global(true),
        )
        .allow_external_subcommands(true)
        .subcommand(
            Command::new("import") // accepts list of files or dirs to import
                .about("Imports music files")
                .arg(arg!(-d --database "Import from preexisting database."))
                .arg(
                    arg!(<PATH> ... "The file(s) or directory(ies) to import.")
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg_required_else_help(true),
        )
        .subcommand(
            Command::new("config") // interact with the config file
                .about("Modify the config file")
                .arg(
                    arg!(-e --edit "Edit the config file with default editor.")
                        .conflicts_with("file"),
                )
                .arg(arg!(-f --file "Return current config file.").conflicts_with("edit")), // later add subcommand set which allows setting of key-value pairs
        )
        .subcommand(
            Command::new("info") // read file metadata
                .about("reads file metadata")
                .arg(
                    arg!(<PATH> ... "The file(s) whose info you want to get.")
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg_required_else_help(true),
        )
}

fn main() -> Result<(), clap::Error> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting radish");

    let mut command = cli();
    let matches = command.try_get_matches_from_mut(std::env::args_os())?;

    // ArgAction::Count produces:
    // -v   => 1
    // -vv  => 2
    // -vvv => 3
    let verbosity = matches.get_count("verbose");

    match matches.subcommand() {
        Some(("import", sub_matches)) => {
            let import_paths = sub_matches
                .get_many::<PathBuf>("PATH")
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();

            let from_database = sub_matches.get_flag("database");
            let dry_run = sub_matches.get_flag("test");

            if from_database && import_paths.len() != 1 {
                return Err(command.error(
                    ErrorKind::WrongNumberOfValues,
                    "`--database` requires exactly one path.",
                ));
            }
            if verbosity > 0 {
                println!("Verbosity level: {verbosity}\nDry run: {dry_run}");
            }

            if from_database {
                println!("Importing database from {:?}", import_paths[0]);
            } else {
                println!("Importing {import_paths:?}");
            }
        }
        Some(("config", sub_matches)) => {
            if sub_matches.get_flag("edit") {
                println!("Editing the config file.");
            } else if sub_matches.get_flag("file") {
                println!("Returning the config file path.");
            } else {
                println!("No config action specified");
            }
        }
        Some(("info", sub_matches)) => {
            let info_paths = sub_matches
                .get_many::<PathBuf>("PATH")
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            println!("getting info about {info_paths:?}");
        }
        Some((ext, sub_matches)) => {
            // Need to figure out what on earth this does
            let args = sub_matches
                .get_many::<OsString>("")
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            println!("Calling out to {ext:?} with {args:?}");
        }
        _ => unreachable!(), // If all subcommands are defined above, anything else is unreachable!()
    }

    Ok(())
}
