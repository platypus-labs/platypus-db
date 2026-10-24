//! `platypus` CLI: runs SQL through the PlatypusDB core and streams the results.

mod output;

use std::io::{Read as _, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use futures::StreamExt;
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult,
    RecordBatchStream,
};
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;

use crate::output::{Format, Sink};

#[derive(Debug, Parser)]
#[command(name = "platypus", version = platypus_core::VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Execute one SQL statement and print or export its result.
    Query(QueryArgs),
}

#[derive(Debug, clap::Args)]
struct QueryArgs {
    /// Connection URL, e.g. `postgres://user:pass@localhost:5432/db?sslmode=disable`,
    /// `mysql://user:pass@localhost:3306/db`, `sqlite:///path/to/file.db` or
    /// `duckdb:///path/to/file.duckdb`
    #[arg(short, long, env = "PLATYPUS_URL", hide_env_values = true)]
    url: String,

    /// SQL to execute; `-` reads it from stdin.
    sql: String,

    #[arg(short, long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,

    /// Write to this file instead of stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Stop after this many rows (the rest of the query is cancelled).
    /// Defaults to 200 for `table`, unlimited otherwise.
    #[arg(long)]
    max_rows: Option<usize>,

    /// Rows per Arrow record batch.
    #[arg(long, default_value_t = ExecOptions::default().batch_size)]
    batch_size: usize,

    /// Cancel the query after this many seconds.
    #[arg(long)]
    timeout: Option<f64>,

    /// Print row count and throughput to stderr.
    #[arg(long)]
    stats: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Csv,
    Ndjson,
    /// A JSON array; needs `--output`.
    Json,
    /// Parquet with zstd; needs `--output`.
    Parquet,
}

/// Writes the result with `platypus-export`, which handles the file formats.
async fn export(
    mut stream: RecordBatchStream,
    path: &std::path::Path,
    format: platypus_export::Format,
    max_rows: Option<usize>,
    stats: bool,
    started: Instant,
) -> Result<(), Error> {
    let options = platypus_export::ExportOptions {
        format,
        ..Default::default()
    };
    let export_error = |e: platypus_export::ExportError| Error::Other(e.to_string());
    let mut exporter = platypus_export::Exporter::create(path, &stream.schema(), &options)
        .map_err(export_error)?;
    let max = max_rows.unwrap_or(usize::MAX);
    let mut rows = 0;
    while rows < max
        && let Some(batch) = stream.next().await
    {
        let batch = batch?;
        let batch = batch.slice(0, batch.num_rows().min(max - rows));
        rows += batch.num_rows();
        exporter.write(&batch).map_err(export_error)?;
    }
    drop(stream);
    exporter.finish().map_err(export_error)?;
    if stats {
        let elapsed = started.elapsed();
        #[allow(clippy::cast_precision_loss)]
        let rate = rows as f64 / elapsed.as_secs_f64();
        eprintln!("{rows} rows in {elapsed:.2?} ({rate:.0} rows/s)");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Query(args) => query(args).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            if let Error::Database(db) = &e
                && let Some(position) = db.position
            {
                eprintln!("at character {position}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn connect(config: &ConnectionConfig) -> Result<Box<dyn Connection>, Error> {
    match config.driver {
        DriverKind::Postgres => PgDriver.connect(config).await,
        DriverKind::MySql => MySqlDriver.connect(config).await,
        DriverKind::Sqlite => SqliteDriver.connect(config).await,
        DriverKind::DuckDb => DuckDbDriver.connect(config).await,
    }
}

async fn query(args: QueryArgs) -> Result<(), Error> {
    let sql = if args.sql == "-" {
        let mut sql = String::new();
        std::io::stdin()
            .read_to_string(&mut sql)
            .map_err(|e| Error::Other(format!("cannot read stdin: {e}")))?;
        sql
    } else {
        args.sql
    };
    let config = ConnectionConfig::from_url(&args.url)?;
    let conn = connect(&config).await?;

    // Ctrl+C and --timeout both cancel server-side, so the error path is exercised for real.
    let cancel = conn.cancel_handle();
    let timeout = args.timeout.map(Duration::from_secs_f64);
    tokio::spawn(async move {
        let deadline = async {
            match timeout {
                Some(t) => tokio::time::sleep(t).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            () = deadline => {}
        }
        let _ = cancel.cancel().await;
    });

    let started = Instant::now();
    let options = ExecOptions {
        batch_size: args.batch_size,
    };
    let mut stream = match conn.execute(&sql, &options).await? {
        ExecResult::Affected { rows } => {
            eprintln!("{rows} row(s) affected in {:.1?}", started.elapsed());
            return Ok(());
        }
        ExecResult::Rows(stream) => stream,
    };

    let format = match args.format {
        OutputFormat::Table => Format::Table,
        OutputFormat::Csv => Format::Csv,
        OutputFormat::Ndjson => Format::Ndjson,
        OutputFormat::Json | OutputFormat::Parquet => {
            let path = args
                .output
                .as_deref()
                .ok_or_else(|| Error::Config("json and parquet output need --output".to_owned()))?;
            let format = if matches!(args.format, OutputFormat::Json) {
                platypus_export::Format::Json
            } else {
                platypus_export::Format::Parquet
            };
            return export(stream, path, format, args.max_rows, args.stats, started).await;
        }
    };
    let max_rows = args
        .max_rows
        .or(matches!(format, Format::Table).then_some(200));
    let writer: Box<dyn Write + Send> = match &args.output {
        Some(path) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(path)
                .map_err(|e| Error::Other(format!("cannot create {}: {e}", path.display())))?,
        )),
        None => Box::new(std::io::BufWriter::with_capacity(
            1 << 16,
            std::io::stdout(),
        )),
    };
    let mut sink = Sink::new(format, &stream.schema(), writer);

    let mut rows = 0usize;
    let mut truncated = false;
    while let Some(batch) = stream.next().await {
        let mut batch = batch?;
        if let Some(max) = max_rows
            && rows + batch.num_rows() >= max
        {
            truncated = rows + batch.num_rows() > max || stream.next().await.is_some();
            batch = batch.slice(0, max - rows);
            rows += batch.num_rows();
            sink.write(&batch)?;
            break;
        }
        rows += batch.num_rows();
        sink.write(&batch)?;
    }
    // Dropping the stream cancels whatever is left of a truncated result.
    drop(stream);
    sink.finish()?;

    let elapsed = started.elapsed();
    if truncated {
        eprintln!("(stopped after {rows} rows; use --max-rows to change)");
    }
    if args.stats {
        #[allow(clippy::cast_precision_loss)]
        let rate = rows as f64 / elapsed.as_secs_f64();
        eprintln!("{rows} rows in {elapsed:.2?} ({rate:.0} rows/s)");
    }
    Ok(())
}
