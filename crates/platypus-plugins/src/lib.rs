//! WebAssembly plugins: commands for the editor, the result grid and the navigator, and
//! drivers for databases the app does not know.
//!
//! A plugin is a package with a manifest ([`manifest`]) and a WebAssembly module. The module
//! runs in an interpreter ([`wasmi`]) with no access to anything but the JSON messages the
//! host gives it and the host functions its approved permissions open: HTTP to the hosts it
//! named, and a log. Every call has a fuel budget, a memory cap and a deadline; running out
//! of any of them, or a trap, is an error of that call and never takes the app down. The
//! guest ABI is described in `docs/PLUGINS.md`; [`runtime`] implements the host side.
//!
//! [`package`] installs, lists, enables and removes plugins under `<data>/plugins`,
//! [`action`] runs their commands and [`driver`] opens their connections.

pub mod action;
pub mod driver;
mod http;
pub mod manifest;
pub mod package;
pub mod runtime;
#[cfg(test)]
mod testing;

use std::sync::OnceLock;

pub use action::{ActionInfo, ActionOutcome, ActionScope};
pub use driver::{DriverInfo, PLUGIN_OPTION, PluginDriver};
pub use manifest::{Kind, Manifest, Permissions};
pub use package::{Inspected, Installed, Plugins};
pub use runtime::{Description, Limits};

/// The guest ABI this host speaks; a manifest names the one its module was written for.
pub const API_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// A package, manifest or module that cannot be used.
    #[error("invalid plugin: {0}")]
    Invalid(String),
    #[error("no plugin {0:?} is installed")]
    NotInstalled(String),
    #[error("the plugin {0:?} is turned off")]
    Disabled(String),
    /// The guest ran out of its fuel: an endless loop, or too much work for one call.
    #[error("the plugin used up its time for this call and was stopped")]
    OutOfFuel,
    #[error("the plugin ran past its deadline and was stopped")]
    Timeout,
    /// The guest crashed (`unreachable`, an access out of its memory, a failed allocation).
    #[error("the plugin stopped with an error: {0}")]
    Trap(String),
    /// The guest answered with an error of its own.
    #[error("{0}")]
    Guest(String),
    /// Something the plugin asked for and was not granted.
    #[error("not allowed: {0}")]
    Denied(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl PluginError {
    pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

impl From<PluginError> for platypus_core::Error {
    fn from(e: PluginError) -> Self {
        match e {
            PluginError::NotInstalled(_)
            | PluginError::Disabled(_)
            | PluginError::Invalid(_)
            | PluginError::Denied(_) => Self::Config(e.to_string()),
            _ => Self::Other(e.to_string()),
        }
    }
}

/// Where the guests' `log` lines go: the level (`ERROR`, `WARN`, `INFO`) and the line,
/// prefixed with the plugin's id.
pub type LogSink = fn(&str, &str);

static LOG_SINK: OnceLock<LogSink> = OnceLock::new();

/// Sends the plugins' log lines to `sink`, e.g. the app's log; once, at start. Without a
/// sink they are dropped.
pub fn set_log_sink(sink: LogSink) {
    let _ = LOG_SINK.set(sink);
}

pub(crate) fn log(level: &str, line: &str) {
    if let Some(sink) = LOG_SINK.get() {
        sink(level, line);
    }
}
