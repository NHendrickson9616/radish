use clap::{Arg, ArgAction, Command, arg, error::ErrorKind};
use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

mod audio_hash;
mod config;
mod data_model;
mod database;
mod import;
mod merge;
mod metadata;
mod operation_model;
mod opus;
mod save_to_db;
mod scan_for_merges;

use config::{Config, ConfigApply, config_path};
use import::{ScanDirAndFiles, import_db};
use operation_model::{Coordinator, Fields};

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
                    Arg::new("max-depth")
                        .short('m')
                        .long("max-depth")
                        .value_name("DEPTH")
                        .help("Maximum depth to traverse for audio files.")
                        .value_parser(clap::value_parser!(usize))
                        .conflicts_with("database"),
                )
                .arg(
                    Arg::new("follow-symlinks")
                        .short('s')
                        .long("follow-symlinks")
                        .help("Follow symlinks when traversing directories.")
                        .action(ArgAction::SetTrue)
                        .conflicts_with("database"),
                )
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
                        .conflicts_with("path"),
                )
                .arg(arg!(-p --path "Return current config file.").conflicts_with("edit")), // later add subcommand set which allows setting of key-value pairs
        )
        .subcommand(
            Command::new("database") // interact with the SQLite database
                .about("Inspect the SQLite database")
                .arg(arg!(-e --edit "Open the database with sqlite3.").conflicts_with("path"))
                .arg(arg!(-p --path "Print the current database path.").conflicts_with("edit")),
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

fn initialize(config: &Config) -> Result<operation_model::Fields, Box<dyn Error>> {
    if !config.library.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "music library directory does not exist: {}",
                config.library.display()
            ),
        )
        .into());
    }

    if config.database.exists() && !config.database.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("database path is not a file: {}", config.database.display()),
        )
        .into());
    }

    if let Some(parent) = config.database.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        if parent.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "database directory path is not a directory: {}",
                    parent.display()
                ),
            )
            .into());
        }

        if !config.dry_run {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut fields = operation_model::Fields::default();
    fields.insert("radish.database", serde_yml::to_value(&config.database)?);
    fields.insert("radish.library", serde_yml::to_value(&config.library)?);
    fields.insert("radish.dry_run", serde_yml::to_value(config.dry_run)?);
    fields.insert(
        "radish.max_depth",
        serde_yml::to_value(config.import.max_depth)?,
    );
    fields.insert(
        "radish.follow_symlinks",
        serde_yml::to_value(config.import.follow_symlinks)?,
    );

    Ok(fields)
}

fn edit_config(path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into());
    let status = ProcessCommand::new(editor).arg(path).status()?;
    if !status.success() {
        return Err(
            std::io::Error::other(format!("config editor exited with status {status}")).into(),
        );
    }

    Ok(())
}

fn edit_database(path: &Path) -> Result<(), Box<dyn Error>> {
    let status = ProcessCommand::new("sqlite3").arg(path).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("sqlite3 exited with status {status}")).into());
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting radish");

    let mut command = cli();
    let matches = command.try_get_matches_from_mut(std::env::args_os())?;

    // ArgAction::Count produces:
    // -v   => 1
    // -vv  => 2
    // -vvv => 3
    let verbosity = matches.get_count("verbose");
    let user_config = config_path()?;

    if let Some(("config", sub_matches)) = matches.subcommand()
        && sub_matches.get_flag("path")
    {
        println!("{}", user_config.display());
        return Ok(());
    }

    let user_values = user_config.is_file().then_some(user_config.as_path());
    let mut config = Config::load(user_values)?;
    config.apply_cli((None, None, matches.get_flag("test").then_some(true)));

    match matches.subcommand() {
        Some(("import", sub_matches)) => {
            let paths = sub_matches
                .get_many::<PathBuf>("PATH")
                .into_iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>();

            let from_database = sub_matches.get_flag("database");

            config.import.apply_cli((
                sub_matches.get_one::<usize>("max-depth").copied(),
                sub_matches.get_flag("follow-symlinks").then_some(true),
            ));

            let shared_fields = initialize(&config)?;

            if from_database && paths.len() != 1 {
                return Err(command
                    .error(
                        ErrorKind::WrongNumberOfValues,
                        "`--database` requires exactly one path.",
                    )
                    .into());
            }

            if verbosity > 0 {
                println!("Verbosity level: {verbosity}\nDry run: {}", config.dry_run);
            }

            if from_database {
                println!("Importing database from {:?}", paths[0]);
                import_db(&paths[0], &config.import)?;
            } else {
                println!("Importing {paths:?}");

                let mut initial_fields = Fields::default();
                initial_fields.insert("radish.paths", serde_yml::to_value(paths)?);

                let mut coordinator = Coordinator::new(shared_fields);
                coordinator
                    .enqueue(Box::new(ScanDirAndFiles), initial_fields)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                coordinator
                    .run()
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            }
        }
        Some(("config", sub_matches)) => {
            if sub_matches.get_flag("edit") {
                edit_config(&user_config)?;
            } else if sub_matches.get_flag("path") {
                unreachable!("config --path returns before configuration loading");
            } else {
                println!("No config action specified");
            }
        }
        Some(("database", sub_matches)) => {
            if sub_matches.get_flag("edit") {
                edit_database(&config.database)?;
            } else if sub_matches.get_flag("path") {
                println!("{}", config.database.display());
            } else {
                println!("No database action specified");
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
