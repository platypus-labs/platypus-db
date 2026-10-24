//! The open databases of the process.
//!
//! DuckDB locks a database file for the instance that opens it, so a second instance on the
//! same file fails even within one process. The app opens several sessions per saved
//! connection (navigator, editor tabs), so every connection to a file shares one instance,
//! kept open while any of them is. `:memory:` is shared the same way: the sessions of the
//! app see the same in-memory tables.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use platypus_core::{Error, Result};

use crate::lock;

/// An open DuckDB instance. New connections are cloned from `root`.
pub struct Database {
    root: Mutex<duckdb::Connection>,
}

static OPEN: Mutex<Vec<(PathBuf, Weak<Database>)>> = Mutex::new(Vec::new());

const MEMORY: &str = ":memory:";

/// Where `path` lives, the same for every spelling of it; the file may not exist yet.
fn key(path: &str) -> Result<PathBuf> {
    if path == MEMORY {
        return Ok(PathBuf::from(MEMORY));
    }
    let path = Path::new(path);
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| Error::Config(format!("{} is not a file", path.display())))?;
    let dir = dir
        .canonicalize()
        .map_err(|_| Error::Connect(format!("{} does not exist", dir.display())))?;
    Ok(dir.join(name))
}

/// Settings of every instance: extensions are neither downloaded nor loaded on their own.
/// The bundled ones (Parquet, JSON) are always there; others need `INSTALL` and `LOAD`.
fn settings() -> Result<duckdb::Config> {
    duckdb::Config::default()
        .enable_autoload_extension(false)
        .and_then(|c| c.with("autoinstall_known_extensions", "false"))
        .map_err(|e| Error::Connect(format!("cannot configure DuckDB: {e}")))
}

/// A new connection to the database at `path` (a file, created when missing, or
/// `:memory:`), with the instance it belongs to. Keep the instance at least as long as the
/// connection, so that a reconnect finds it open.
pub fn connect(path: &str) -> Result<(Arc<Database>, duckdb::Connection)> {
    if path.is_empty() {
        return Err(Error::Config("no database file given".to_owned()));
    }
    let key = key(path)?;
    let mut open = lock(&OPEN);
    open.retain(|(_, db)| db.strong_count() > 0);
    let existing = open
        .iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, db)| db.upgrade());
    let database = if let Some(database) = existing {
        database
    } else {
        let root = if path == MEMORY {
            duckdb::Connection::open_in_memory_with_flags(settings()?)
        } else {
            duckdb::Connection::open_with_flags(&key, settings()?)
        }
        .map_err(|e| Error::Connect(format!("cannot open {path}: {e}")))?;
        let database = Arc::new(Database {
            root: Mutex::new(root),
        });
        open.push((key, Arc::downgrade(&database)));
        database
    };
    drop(open);
    let conn = lock(&database.root)
        .try_clone()
        .map_err(|e| Error::Connect(format!("cannot connect to {path}: {e}")))?;
    Ok((database, conn))
}
