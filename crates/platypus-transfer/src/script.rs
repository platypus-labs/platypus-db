//! Runs an SQL file, of any size, on a connection: read a piece at a time (gzip and zstd
//! too), split into statements as the database's dialect writes them (Transact-SQL in
//! batches up to `GO`), and run in order. The last statement of what has been read waits for
//! the rest of the file, which may go on with it.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;
use platypus_core::{Connection, ExecOptions, ExecResult};
use platypus_sql::Dialect;

/// How much of the file is read at a time.
const CHUNK: usize = 4 << 20;
/// How much of a failed statement the error shows.
const SHOWN: usize = 200;

/// How far a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptProgress {
    /// Bytes of the file read, compressed ones for a compressed file.
    pub read: u64,
    pub total: u64,
    /// Statements (batches in Transact-SQL) run.
    pub statements: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("cannot read {path}: {message}")]
    Read { path: String, message: String },
    /// A statement failed: its number, its line in the file and its start.
    #[error("Statement {number} (line {line}) failed: {error}\n{sql}")]
    Statement {
        number: u64,
        line: u64,
        sql: String,
        error: platypus_core::Error,
    },
    #[error("{0}")]
    Database(#[from] platypus_core::Error),
    #[error("the script was stopped")]
    Cancelled,
}

/// Runs the file at `path` on `conn`. With `transaction` (and a database that has them) it
/// all runs in one, which a failure or a stop rolls back; else what ran before stays.
/// `progress` is told after each statement and returns false to stop. Returns how many
/// statements ran.
///
/// # Errors
/// When the file cannot be read, a statement fails, or `progress` stops the run.
pub async fn run_file(
    conn: &dyn Connection,
    dialect: Dialect,
    path: &Path,
    transaction: bool,
    mut progress: impl FnMut(ScriptProgress) -> bool,
) -> Result<u64, ScriptError> {
    let mut source = Source::open(path)?;
    let transaction = transaction && conn.capabilities().transactions;
    if transaction {
        run(conn, "BEGIN").await?;
    }
    let ran = run_all(conn, dialect, &mut source, &mut progress).await;
    if transaction {
        match &ran {
            Ok(_) => conn.commit().await?,
            // The first error explains more than a failed rollback.
            Err(_) => {
                let _ = conn.rollback().await;
            }
        }
    }
    ran
}

async fn run_all(
    conn: &dyn Connection,
    dialect: Dialect,
    source: &mut Source,
    progress: &mut impl FnMut(ScriptProgress) -> bool,
) -> Result<u64, ScriptError> {
    let mut buffer = String::new();
    // Lines before the start of `buffer`.
    let mut line = 0u64;
    let mut number = 0u64;
    loop {
        let eof = source.read_into(&mut buffer)?;
        let units = units(&buffer, dialect);
        // The last one may go on in what is not read yet.
        let ready = if eof {
            units.len()
        } else {
            units.len().saturating_sub(1)
        };
        for unit in &units[..ready] {
            number += 1;
            let sql = &buffer[unit.start..unit.end];
            for _ in 0..unit.repeat {
                if let Err(error) = run(conn, sql).await {
                    let at = line + buffer[..unit.start].matches('\n').count() as u64 + 1;
                    return Err(ScriptError::Statement {
                        number,
                        line: at,
                        sql: shown(sql),
                        error,
                    });
                }
            }
            let now = ScriptProgress {
                read: source.read(),
                total: source.total,
                statements: number,
            };
            if !progress(now) {
                return Err(ScriptError::Cancelled);
            }
        }
        if eof {
            return Ok(number);
        }
        let consumed = units.get(ready).map_or(buffer.len(), |u| u.start);
        line += buffer[..consumed].matches('\n').count() as u64;
        buffer.drain(..consumed);
    }
}

/// A statement, or a Transact-SQL batch run `repeat` times, by its range in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Unit {
    start: usize,
    end: usize,
    repeat: u32,
}

fn units(text: &str, dialect: Dialect) -> Vec<Unit> {
    let statements = platypus_sql::split(text, dialect);
    if dialect != Dialect::SqlServer {
        return statements
            .iter()
            .map(|s| Unit {
                start: s.start,
                end: s.end,
                repeat: 1,
            })
            .collect();
    }
    platypus_sql::batches(text, dialect)
        .into_iter()
        .filter(|b| !b.statements.is_empty())
        .map(|b| Unit {
            start: statements[b.statements.start].start,
            end: statements[b.statements.end - 1].end,
            repeat: b.repeat,
        })
        .collect()
}

/// Runs `sql` and reads what it returns, which a script has no use for.
async fn run(conn: &dyn Connection, sql: &str) -> Result<(), platypus_core::Error> {
    if let ExecResult::Rows(mut rows) = conn.execute(sql, &ExecOptions::default()).await? {
        while let Some(batch) = rows.next().await {
            batch?;
        }
    }
    Ok(())
}

/// The start of a failed statement, on one line.
fn shown(sql: &str) -> String {
    let one: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(SHOWN) {
        Some((at, _)) => format!("{}…", &one[..at]),
        None => one,
    }
}

/// The file's text a piece at a time, decompressed by its extension (`.gz`, `.zst`).
struct Source {
    reader: Box<dyn Read + Send>,
    /// Bytes of the file read so far, before decompression.
    read: Arc<AtomicU64>,
    total: u64,
    path: String,
    /// The start of a character cut at the end of the last piece.
    pending: Vec<u8>,
}

impl Source {
    fn open(path: &Path) -> Result<Self, ScriptError> {
        let failed = |e: std::io::Error| ScriptError::Read {
            path: path.display().to_string(),
            message: e.to_string(),
        };
        let file = File::open(path).map_err(failed)?;
        let total = file.metadata().map_err(failed)?.len();
        let read = Arc::new(AtomicU64::new(0));
        let counted = Counted {
            inner: file,
            read: Arc::clone(&read),
        };
        let name = path.to_string_lossy().to_ascii_lowercase();
        let reader: Box<dyn Read + Send> = if name.ends_with(".gz") {
            Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(counted)))
        } else if name.ends_with(".zst") {
            Box::new(zstd::Decoder::new(counted).map_err(failed)?)
        } else {
            Box::new(BufReader::new(counted))
        };
        Ok(Self {
            reader,
            read,
            total,
            path: path.display().to_string(),
            pending: Vec::new(),
        })
    }

    fn read(&self) -> u64 {
        self.read.load(Ordering::Relaxed)
    }

    /// Adds the next piece to `text`; true at the end of the file.
    fn read_into(&mut self, text: &mut String) -> Result<bool, ScriptError> {
        let mut bytes = std::mem::take(&mut self.pending);
        let start = bytes.len();
        bytes.resize(start + CHUNK, 0);
        let mut filled = start;
        while filled < bytes.len() {
            match self.reader.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    return Err(ScriptError::Read {
                        path: self.path.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }
        bytes.truncate(filled);
        let eof = filled < start + CHUNK;
        match std::str::from_utf8(&bytes) {
            Ok(s) => text.push_str(s),
            // A character cut in two waits for its other half; bytes that are not UTF-8 at
            // all become U+FFFD.
            Err(e) if e.error_len().is_none() && !eof => {
                let (whole, cut) = bytes.split_at(e.valid_up_to());
                text.push_str(&String::from_utf8_lossy(whole));
                self.pending = cut.to_vec();
            }
            Err(_) => text.push_str(&String::from_utf8_lossy(&bytes)),
        }
        Ok(eof)
    }
}

/// A reader that counts the bytes it gives.
struct Counted {
    inner: File,
    read: Arc<AtomicU64>,
}

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_and_batches() {
        let pg = "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1; $$ LANGUAGE sql;\nSELECT f();";
        let u = units(pg, Dialect::Postgres);
        assert_eq!(u.len(), 2, "the function's body is one statement");
        assert_eq!(&pg[u[1].start..u[1].end], "SELECT f()");
        let tsql = "DECLARE @a int = 1\nSELECT @a\nGO 2\nSELECT 2\nGO\n";
        let u = units(tsql, Dialect::SqlServer);
        assert_eq!(u.len(), 2);
        assert_eq!(&tsql[u[0].start..u[0].end], "DECLARE @a int = 1\nSELECT @a");
        assert_eq!((u[0].repeat, u[1].repeat), (2, 1));
    }

    #[test]
    fn a_failed_statement_shows_on_one_line() {
        assert_eq!(shown("SELECT\n  1,\n  2"), "SELECT 1, 2");
        let long = "x".repeat(300);
        assert_eq!(shown(&long).chars().count(), SHOWN + 1);
    }

    #[test]
    fn reads_compressed_files_and_cut_characters() {
        let dir = tempfile::tempdir().unwrap();
        let text = "SELECT 'é';\n".repeat(1000);
        let gz = dir.path().join("s.sql.gz");
        let mut encoder =
            flate2::write::GzEncoder::new(File::create(&gz).unwrap(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, text.as_bytes()).unwrap();
        encoder.finish().unwrap();
        let mut source = Source::open(&gz).unwrap();
        let mut read = String::new();
        while !source.read_into(&mut read).unwrap() {}
        assert_eq!(read, text);
        assert_eq!(source.read(), source.total);
    }
}
