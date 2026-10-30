//! Two ways to share PlatypusDB's data without a server, both through a plain folder:
//!
//! - **Sync** ([`file`], [`merge`], [`collect`]): settings, connections without passwords,
//!   projects and scripts, kept in one end-to-end encrypted file (`platypus-sync.bin`) in a
//!   folder the user picks (a cloud drive, a network share, a Git checkout). Each machine
//!   merges the file with its own data item by item.
//! - **Shared projects** ([`shared`]): a project written as a readable folder
//!   (`platypus-project.toml` and `scripts/`) a team keeps in Git or on a shared drive.
//!
//! Neither ever writes a credential: connections go through [`portable`], which has a field
//! for everything that may leave the machine and none for anything else.

pub mod collect;
pub mod crypto;
pub mod file;
pub mod merge;
pub mod model;
pub mod portable;
pub mod shared;

pub use crypto::SyncError;

pub type Result<T, E = SyncError> = std::result::Result<T, E>;
