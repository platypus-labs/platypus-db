//! Analysis of result rows on the user's machine: the profile of a column, the data of a
//! chart, the differences between two runs, and a DuckDB database filled with rows to query
//! them with SQL. No UI and no Tauri: the desktop app calls it with a result's Arrow batches.

pub mod chart;
pub mod diff;
pub mod duck;
pub mod patterns;
pub mod stats;
pub mod table_profile;
pub mod values;

#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error("the result has no column {}", .0 + 1)]
    NoColumn(usize),
    #[error("the two results do not have the same columns")]
    ColumnsDiffer,
    #[error(transparent)]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error("DuckDB: {0}")]
    Duck(String),
}
