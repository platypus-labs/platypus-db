//! The open databases of the process.
//!
//! DuckDB locks a database file for the instance that opens it, so a second instance on the
//! same file fails even within one process. The app opens several sessions per saved
//! connection (navigator, editor tabs), so every connection to a file shares one instance,
//! kept open while any of them is. `:memory:` is shared the same way: the sessions of the
//! app see the same in-memory tables. `:memory:<name>` is an in-memory database of its own,
//! shared the same way (see `crate::memory_database`). A data file (CSV, Parquet, XLSX...)
//! gets an in-memory instance of its own, shared the same way, that reads it
//! ([`crate::files`]); a new connection loads it again once the file changed, which matters
//! for the copied sheets of a workbook.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::SystemTime;

use platypus_core::{Error, Result};

use crate::{files, lock};

/// An open DuckDB instance. New connections are cloned from `root`.
pub struct Database {
    /// `None` only while it is dropped.
    root: Mutex<Option<duckdb::Connection>>,
    /// When the data file it reads was last changed, as it was loaded; `None` otherwise.
    loaded: Mutex<Option<SystemTime>>,
}

impl Database {
    fn root(&self) -> std::sync::MutexGuard<'_, Option<duckdb::Connection>> {
        lock(&self.root)
    }
}

impl Drop for Database {
    /// Closes the instance while holding the list of open ones: a connect that no longer
    /// finds it there waits until DuckDB has released the file, instead of failing to open
    /// it meanwhile.
    fn drop(&mut self) {
        let _open = lock(&OPEN);
        drop(self.root().take());
    }
}

/// The open instances, by path and by how a data file is read.
static OPEN: Mutex<Vec<(PathBuf, files::ReadOptions, Weak<Database>)>> = Mutex::new(Vec::new());

const MEMORY: &str = ":memory:";

/// Where `path` lives, the same for every spelling of it; the file may not exist yet.
fn key(path: &str) -> Result<PathBuf> {
    if path.starts_with(MEMORY) {
        return Ok(PathBuf::from(path));
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
/// connection, so that a reconnect finds it open. A data file read with other `options` is
/// another instance.
///
/// With `read_only` (a read-only connection), a database file not open yet is opened
/// read-only, so that DuckDB itself refuses writes. One file has one instance in the
/// process: a connection to a file already open shares its instance as it was opened, and an
/// in-memory database or a data file is never read-only (the statement check stops writes).
pub fn connect(
    path: &str,
    options: &files::ReadOptions,
    read_only: bool,
) -> Result<(Arc<Database>, duckdb::Connection)> {
    if path.is_empty() {
        return Err(Error::Config("no database file given".to_owned()));
    }
    crate::library::load()?;
    let key = key(path)?;
    let mut open = lock(&OPEN);
    open.retain(|(_, _, db)| db.strong_count() > 0);
    let existing = open
        .iter()
        .find(|(k, o, _)| *k == key && o == options)
        .and_then(|(_, _, db)| db.upgrade());
    // `:memory:<name>` is never a data file, whatever its name ends with.
    let data_file = !path.starts_with(MEMORY) && files::is_data_file(path);
    let database = if let Some(database) = existing {
        database
    } else {
        let root = if path.starts_with(MEMORY) || data_file {
            duckdb::Connection::open_in_memory_with_flags(settings()?)
        } else if read_only {
            let settings = settings()?
                .access_mode(duckdb::AccessMode::ReadOnly)
                .map_err(|e| Error::Connect(format!("cannot configure DuckDB: {e}")))?;
            duckdb::Connection::open_with_flags(&key, settings)
        } else {
            duckdb::Connection::open_with_flags(&key, settings()?)
        }
        .map_err(|e| Error::Connect(format!("cannot open {path}: {e}")))?;
        let database = Arc::new(Database {
            root: Mutex::new(Some(root)),
            loaded: Mutex::new(None),
        });
        open.push((key, options.clone(), Arc::downgrade(&database)));
        database
    };
    drop(open);
    if data_file {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut loaded = lock(&database.loaded);
        if loaded.is_none() || *loaded != modified {
            if let Some(root) = &*database.root() {
                files::load(root, path, options)?;
            }
            *loaded = modified;
        }
    }
    let conn = database
        .root()
        .as_ref()
        .ok_or(Error::Closed)?
        .try_clone()
        .map_err(|e| Error::Connect(format!("cannot connect to {path}: {e}")))?;
    Ok((database, conn))
}
