//! SQLite database support for Radish.

use std::fmt;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

pub const INITIAL_SCHEMA: &str = include_str!("../migrations/001_initial.sql");

const APPLICATION_ID: i64 = 1_380_009_033; // ASCII "RADI"
const SCHEMA_VERSION: i64 = 1;
const REQUIRED_TABLES: &[&str] = &[
    "compositions",
    "files",
    "recordings",
    "release_tracks",
    "releases",
    "versions",
];

pub type DatabaseResult<T> = Result<T, DatabaseError>;

#[cfg(unix)]
pub fn path_to_blob(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
pub fn path_to_blob(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[derive(Debug)]
pub enum DatabaseError {
    Sqlite(rusqlite::Error),
    InvalidSchema(String),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "database error: {error}"),
            Self::InvalidSchema(message) => write!(formatter, "invalid database schema: {message}"),
        }
    }
}

impl std::error::Error for DatabaseError {}

impl From<rusqlite::Error> for DatabaseError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Opens a Radish database, creating its schema when the database is empty.
///
/// Existing databases are never modified here. They must have Radish's application ID,
/// supported schema version, and all required tables.
pub fn initialize(path: &Path) -> DatabaseResult<Connection> {
    let mut connection = Connection::open(path)?;
    initialize_connection(&mut connection)?;
    Ok(connection)
}

pub fn open_for_scan(path: &Path, dry_run: bool) -> DatabaseResult<Connection> {
    if !dry_run {
        return initialize(path);
    }

    if !path.exists() {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(INITIAL_SCHEMA)?;
        return Ok(connection);
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    validate_schema(&connection)?;
    Ok(connection)
}

fn initialize_connection(connection: &mut Connection) -> DatabaseResult<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;

    let table_count: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;

    if table_count == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(INITIAL_SCHEMA)?;
        transaction.commit()?;
    }

    validate_schema(connection)
}

fn validate_schema(connection: &Connection) -> DatabaseResult<()> {
    let application_id: i64 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(DatabaseError::InvalidSchema(format!(
            "expected Radish application ID {APPLICATION_ID}, found {application_id}"
        )));
    }

    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(DatabaseError::InvalidSchema(format!(
            "expected schema version {SCHEMA_VERSION}, found {version}"
        )));
    }

    for table in REQUIRED_TABLES {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(DatabaseError::InvalidSchema(format!(
                "required table `{table}` is missing"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_an_empty_database() {
        let mut connection = Connection::open_in_memory().unwrap();

        initialize_connection(&mut connection).unwrap();

        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, REQUIRED_TABLES.len() as i64);
    }

    #[test]
    fn accepts_zero_release_positions() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_connection(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO compositions VALUES (1, 'Track', 0, 0);
                 INSERT INTO recordings VALUES (1, 1, 'Track', 0, 0);
                 INSERT INTO versions VALUES (1, 1, 'unknown', NULL, 0, 0);
                 INSERT INTO releases VALUES (1, 'Release', 0, 0);
                 INSERT INTO release_tracks VALUES (1, 1, 1, 0, 0, 'Track', 0, 0);",
            )
            .unwrap();
    }

    #[test]
    fn accepts_an_existing_valid_database() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_connection(&mut connection).unwrap();

        initialize_connection(&mut connection).unwrap();
    }

    #[test]
    fn rejects_a_foreign_database() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute("CREATE TABLE items (id INTEGER)", [])
            .unwrap();

        let error = initialize_connection(&mut connection).unwrap_err();

        assert!(error.to_string().contains("application ID"));
    }

    #[test]
    fn rejects_an_unsupported_schema_version() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_connection(&mut connection).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 2;")
            .unwrap();

        let error = initialize_connection(&mut connection).unwrap_err();

        assert!(error.to_string().contains("schema version"));
    }

    #[test]
    fn rejects_a_missing_required_table() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_connection(&mut connection).unwrap();
        connection.execute("DROP TABLE release_tracks", []).unwrap();

        let error = initialize_connection(&mut connection).unwrap_err();

        assert!(error.to_string().contains("release_tracks"));
    }

    #[test]
    fn dry_run_scan_does_not_create_a_missing_database_file() {
        let path = std::env::temp_dir().join(format!(
            "radish-dry-run-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let connection = open_for_scan(&path, true).unwrap();

        assert!(!path.exists());
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, REQUIRED_TABLES.len() as i64);
    }
}
