//! Core abstractions shared by every PlatypusDB driver and frontend.
//!
//! Populated in M1: `Driver`, `Connection`, `MetadataProvider`, `ResultStream`.

/// Version of the PlatypusDB core, shared by the CLI and the desktop app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
