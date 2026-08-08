use clap::{Command, Error, arg};
use std::ffi::OsString;
use std::path::PathBuf;

fn cli() -> Command {
    Command::new("rad")
        .about("A music management tool")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .allow_external_subcommands(true)
        .subcommand(
            Command::new("import") // accepts list of files or dirs to import
                .about("Imports music files")
                .arg(
                    arg!(<PATH> ... "The file(s) or directory(ies) to import.")
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg_required_else_help(true),
        )
        .subcommand(
            Command::new("config") // interact with the config file
                .about("Modify the config file")
                .arg(arg!(-e --edit "Edit the config file with default editor"))
                .arg(arg!(-f --file "Return current config file")),
            // later add subcommand set which allows setting of key-value pairs
        )
        .subcommand(
            Command::new("getinfo") // read file metadata
                .about("reads file metadata")
                .arg(
                    arg!(<PATH> ... "The file(s) whose info you want to get.")
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg_required_else_help(true),
        )
}

fn main() -> Result<(), Error> {
    let matches = cli().try_get_matches()?;

    match matches.subcommand() {
        Some(("import", sub_matches)) => {
            let import_paths = sub_matches
                .get_many::<PathBuf>("PATH")
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            println!("Importing {import_paths:?}");
        }
        Some(("config", _sub_matches)) => {
            // Here I need to test and see what flag was passed as bool essentially...
            println!("config boolean options have not yet been implemented");
        }
        Some(("getinfo", sub_matches)) => {
            let import_paths = sub_matches
                .get_many::<PathBuf>("PATH")
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            println!("Adding {import_paths:?}");
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
