//! The log and the diagnostic package: a zip the user saves and attaches to a bug report,
//! with the app's version and system, its logs and last crash, the settings without anything
//! that looks like a secret, the drivers installed and how many connections of each database
//! there are. No saved connection's name, host, user or password goes in; error messages in
//! the logs may name a server or a table, which the Diagnostics page says.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

use platypus_store::toml;
use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::applog;
use crate::error::{CommandError, CommandResult};

/// Setting keys that may hold a secret, dropped from the package: a part of the dotted key
/// containing one of these.
const SECRET_WORDS: &[&str] = &["password", "secret", "token", "credential", "passphrase"];

#[derive(Debug, Serialize)]
pub struct DiagnosticsInfo {
    /// The folder of the logs.
    log_dir: Option<String>,
    /// The report of the crash that ended the last run, until dismissed.
    crash: Option<String>,
}

#[tauri::command]
pub fn diagnostics_info() -> DiagnosticsInfo {
    DiagnosticsInfo {
        log_dir: applog::dir().map(|d| d.display().to_string()),
        crash: applog::crash(),
    }
}

#[tauri::command]
pub fn dismiss_crash() {
    applog::dismiss_crash();
}

/// An error the web view did not handle, for the log.
#[tauri::command]
pub fn log_error(message: String) {
    const MAX: usize = 4000;
    let cut = message
        .char_indices()
        .nth(MAX)
        .map_or(message.len(), |(i, _)| i);
    applog::write("ERROR", &format!("web view: {}", &message[..cut]));
}

/// `table` without the keys that look like secrets, at any depth, arrays of tables included.
fn without_secrets(table: &toml::Table) -> toml::Table {
    table
        .iter()
        .filter(|(key, _)| {
            let key = key.to_lowercase();
            !SECRET_WORDS.iter().any(|w| key.contains(w))
        })
        .map(|(key, value)| (key.clone(), value_without_secrets(value)))
        .collect()
}

fn value_without_secrets(value: &toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(inner) => toml::Value::Table(without_secrets(inner)),
        toml::Value::Array(items) => {
            toml::Value::Array(items.iter().map(value_without_secrets).collect())
        }
        other => other.clone(),
    }
}

/// What goes in the package besides the logs.
struct Contents {
    about: String,
    settings: String,
    connections: String,
}

fn about(drivers: &[String]) -> String {
    let mut out = format!(
        "PlatypusDB {}\nSystem: {} {} ({})\nWeb view: {}\n\nDrivers downloaded:\n",
        platypus_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY,
        tauri::webview_version().unwrap_or_else(|e| format!("unknown ({e})")),
    );
    if drivers.is_empty() {
        out.push_str("  none\n");
    }
    for driver in drivers {
        let _ = writeln!(out, "  {driver}");
    }
    out
}

/// How many connections of each database, and of each type (dev, test, prod).
fn connection_counts(connections: &[platypus_store::SavedConnection]) -> String {
    let mut drivers = BTreeMap::<&str, usize>::new();
    let mut types = BTreeMap::<String, usize>::new();
    for c in connections {
        *drivers.entry(c.driver.name()).or_default() += 1;
        let kind = c
            .connection_type
            .map_or_else(|| "none".to_owned(), |t| format!("{t:?}").to_lowercase());
        *types.entry(kind).or_default() += 1;
    }
    let mut out = format!("{} saved connections\n\nBy database:\n", connections.len());
    for (driver, n) in drivers {
        let _ = writeln!(out, "  {driver}: {n}");
    }
    out.push_str("\nBy type:\n");
    for (kind, n) in types {
        let _ = writeln!(out, "  {kind}: {n}");
    }
    out
}

/// The downloaded drivers and vendor libraries, as `name version`.
fn downloaded_drivers() -> Vec<String> {
    let Some(dir) = platypus_drivers::data_dir() else {
        return Vec::new();
    };
    let vendors = platypus_drivers::VENDOR_PACKAGES
        .iter()
        .filter_map(|name| platypus_drivers::installed_vendor(dir, name));
    platypus_drivers::list(dir)
        .into_iter()
        .chain(vendors)
        .map(|i| format!("{} {}", i.name, i.version))
        .collect()
}

/// Writes the zip: the contents, then every file of `log_dir` under `logs/`.
fn write_zip(
    out: impl Write + Seek,
    contents: &Contents,
    log_dir: Option<&Path>,
) -> zip::result::ZipResult<()> {
    let mut zip = zip::ZipWriter::new(out);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, text) in [
        ("about.txt", &contents.about),
        ("settings.toml", &contents.settings),
        ("connections.txt", &contents.connections),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(text.as_bytes())?;
    }
    let logs = log_dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file());
    for entry in logs {
        zip.start_file(
            format!("logs/{}", entry.file_name().to_string_lossy()),
            options,
        )?;
        zip.write_all(&std::fs::read(entry.path())?)?;
    }
    zip.finish()?;
    Ok(())
}

/// Saves the diagnostic package at `path`.
#[tauri::command]
pub async fn save_diagnostics(state: State<'_, AppState>, path: PathBuf) -> CommandResult<()> {
    let settings = state.with_settings(|s| Ok(without_secrets(s.all())))?;
    let connections = state.with_store(|s| Ok(connection_counts(s.list())))?;
    let contents = Contents {
        about: about(&downloaded_drivers()),
        settings: toml::to_string(&settings).unwrap_or_default(),
        connections,
    };
    let shown = path.display().to_string();
    let fail = move |e: &dyn std::fmt::Display| {
        CommandError::new("io", format!("cannot write {shown}: {e}"))
    };
    // Off the async runtime, into a file of its own renamed once complete: a failure leaves
    // no half-written zip behind.
    tokio::task::spawn_blocking(move || {
        let partial = path.with_extension("zip.partial");
        let written = std::fs::File::create(&partial)
            .map_err(zip::result::ZipError::Io)
            .and_then(|file| write_zip(file, &contents, applog::dir()))
            .and_then(|()| std::fs::rename(&partial, &path).map_err(zip::result::ZipError::Io));
        if written.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        written.map_err(|e| fail(&e))
    })
    .await
    .map_err(|e| CommandError::new("io", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use platypus_core::DriverKind;
    use platypus_store::SavedConnection;

    use super::*;

    #[test]
    fn drops_secret_settings() {
        let table: toml::Table = toml::from_str(
            r#"
            theme = "dark"
            [proxy]
            host = "proxy.local"
            Password = "hunter2"
            [cloud]
            api_token = "abc"
            [[profiles]]
            name = "work"
            password = "s3cret"
            [[profiles]]
            nested = [{ secret = "deep" }]
            "#,
        )
        .unwrap();
        let text = toml::to_string(&without_secrets(&table)).unwrap();
        assert!(
            text.contains("proxy.local") && text.contains("dark") && text.contains("work"),
            "{text}"
        );
        for secret in ["hunter2", "abc", "s3cret", "deep"] {
            assert!(!text.contains(secret), "{text}");
        }
    }

    #[test]
    fn counts_connections_without_names() {
        let mut prod = SavedConnection::new("Billing prod", DriverKind::Postgres);
        prod.host = "db.internal".to_owned();
        let all = [
            prod,
            SavedConnection::new("Local", DriverKind::Sqlite),
            SavedConnection::new("Other", DriverKind::Postgres),
        ];
        let text = connection_counts(&all);
        assert!(text.starts_with("3 saved connections"), "{text}");
        assert!(text.contains("PostgreSQL: 2"), "{text}");
        assert!(
            !text.contains("Billing") && !text.contains("db.internal"),
            "{text}"
        );
    }

    #[test]
    fn packs_contents_and_logs() {
        let logs = tempfile::tempdir().unwrap();
        std::fs::write(logs.path().join("platypus.log"), "started").unwrap();
        std::fs::write(logs.path().join("crash.txt"), "panicked").unwrap();
        let contents = Contents {
            about: about(&["duckdb 2.0.0".to_owned()]),
            settings: "theme = \"dark\"\n".to_owned(),
            connections: "0 saved connections\n".to_owned(),
        };
        let mut bytes = std::io::Cursor::new(Vec::new());
        write_zip(&mut bytes, &contents, Some(logs.path())).unwrap();
        let mut zip = zip::ZipArchive::new(bytes).unwrap();
        let mut names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "about.txt",
                "connections.txt",
                "logs/crash.txt",
                "logs/platypus.log",
                "settings.toml"
            ]
        );
        let mut about = String::new();
        zip.by_name("about.txt")
            .unwrap()
            .read_to_string(&mut about)
            .unwrap();
        assert!(about.contains("duckdb 2.0.0"), "{about}");
    }
}
