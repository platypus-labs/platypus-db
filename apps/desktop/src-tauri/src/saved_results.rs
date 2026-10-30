//! Saved results: the rows of a result kept in a Parquet file of the data folder, to be
//! opened later without the connection they came from. `saved-results/index.json` lists them
//! (name, query, connection, time, columns); each result's rows are in
//! `saved-results/<id>.parquet`, written by the export code, so a file also opens in any
//! other tool that reads Parquet. The commands are in `commands::saved_results`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use arrow_array::{RecordBatch, RecordBatchReader};
use arrow_schema::{Schema, SchemaRef};
use chrono::{DateTime, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use platypus_core::META_DB_TYPE;
use platypus_transfer::{ExportOptions, Exporter, Format};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A column of a saved result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedColumn {
    pub name: String,
    /// The type the database named (`VARCHAR(20)`, `numeric`), else Arrow's.
    pub data_type: String,
}

/// What the index keeps of a saved result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedResult {
    pub id: Uuid,
    pub name: String,
    /// The statement that produced the rows.
    pub sql: String,
    /// The connection's name and driver when the result was saved: it may be renamed or
    /// deleted since, the saved result does not need it.
    pub connection: String,
    pub driver: String,
    #[serde(default)]
    pub database: Option<String>,
    pub saved_at: DateTime<Utc>,
    pub rows: u64,
    pub columns: Vec<SavedColumn>,
    /// The result had more rows than were saved: reading stopped at the memory limit, or it
    /// was stopped.
    #[serde(default)]
    pub partial: bool,
}

/// What the page tells about a result it saves; the rest is read from the rows.
#[derive(Debug, Clone, Deserialize)]
pub struct Origin {
    pub name: String,
    pub sql: String,
    pub connection: String,
    pub driver: String,
    #[serde(default)]
    pub database: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    results: Vec<SavedResult>,
}

/// The saved results of the data folder.
#[derive(Debug, Clone)]
pub struct SavedResults {
    /// `saved-results` in the data folder; `None` without one.
    dir: Option<PathBuf>,
    /// Shared by the clones, which the commands move to a blocking thread.
    index: Arc<Mutex<Index>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The columns of `schema`, with the database's own type where the driver kept it.
fn columns(schema: &Schema) -> Vec<SavedColumn> {
    schema
        .fields()
        .iter()
        .map(|f| SavedColumn {
            name: f.name().clone(),
            data_type: f
                .metadata()
                .get(META_DB_TYPE)
                .cloned()
                .unwrap_or_else(|| f.data_type().to_string()),
        })
        .collect()
}

/// The index at `path`, if there is one. One that cannot be read is moved aside to
/// `index.json.unreadable` rather than written over.
fn read_index(path: &Path) -> Option<Index> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(index) => Some(index),
        Err(e) => {
            let aside = path.with_extension("json.unreadable");
            let _ = std::fs::rename(path, &aside);
            crate::applog::write(
                "WARN",
                &format!(
                    "saved-results/index.json cannot be read ({e}): moved to {}",
                    aside.display()
                ),
            );
            None
        }
    }
}

impl SavedResults {
    /// The results saved in `dir`; none when it has no index.
    pub fn open(dir: Option<PathBuf>) -> Self {
        let index = dir
            .as_ref()
            .and_then(|d| read_index(&d.join("index.json")))
            .unwrap_or_default();
        Self {
            dir,
            index: Arc::new(Mutex::new(index)),
        }
    }

    fn dir(&self) -> Result<&Path, String> {
        self.dir
            .as_deref()
            .ok_or_else(|| "the app has no data folder to keep results in".to_owned())
    }

    fn file(&self, id: Uuid) -> Result<PathBuf, String> {
        Ok(self.dir()?.join(format!("{id}.parquet")))
    }

    /// Writes the index aside, then moves it: a crash halfway leaves the old one whole.
    fn write(&self, index: &Index) -> Result<(), String> {
        let path = self.dir()?.join("index.json");
        let text = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
        let aside = path.with_extension("json.tmp");
        std::fs::write(&aside, text)
            .and_then(|()| std::fs::rename(&aside, &path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The saved results, the latest first.
    pub fn list(&self) -> Vec<SavedResult> {
        let mut all = lock(&self.index).results.clone();
        all.sort_by_key(|r| std::cmp::Reverse(r.saved_at));
        all
    }

    /// The saved result `id`.
    pub fn get(&self, id: Uuid) -> Result<SavedResult, String> {
        lock(&self.index)
            .results
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| "the saved result is gone".to_owned())
    }

    /// Keeps `batches` (with `schema`) as a new saved result from `origin`; `partial` when
    /// they are not all of the result's rows.
    pub fn add(
        &self,
        origin: Origin,
        schema: &Schema,
        batches: &[RecordBatch],
        partial: bool,
    ) -> Result<SavedResult, String> {
        let name = origin.name.trim();
        if name.is_empty() {
            return Err("the saved result needs a name".to_owned());
        }
        let dir = self.dir()?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let id = Uuid::new_v4();
        let path = self.file(id)?;
        let rows = write_rows(&path, schema, batches)?;
        let saved = SavedResult {
            id,
            name: name.to_owned(),
            sql: origin.sql,
            connection: origin.connection,
            driver: origin.driver,
            database: origin.database,
            saved_at: Utc::now(),
            rows,
            columns: columns(schema),
            partial,
        };
        let mut index = lock(&self.index);
        index.results.push(saved.clone());
        if let Err(e) = self.write(&index) {
            index.results.pop();
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        Ok(saved)
    }

    /// Renames the saved result `id`.
    pub fn rename(&self, id: Uuid, name: &str) -> Result<SavedResult, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("the saved result needs a name".to_owned());
        }
        let mut index = lock(&self.index);
        let entry = index
            .results
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or("the saved result is gone")?;
        let before = std::mem::replace(&mut entry.name, name.to_owned());
        let renamed = entry.clone();
        if let Err(e) = self.write(&index) {
            if let Some(entry) = index.results.iter_mut().find(|r| r.id == id) {
                entry.name = before;
            }
            return Err(e);
        }
        Ok(renamed)
    }

    /// Deletes the saved result `id` and its file. The index goes first: a file left behind
    /// by a failure is only wasted space, an entry without its file would not open.
    pub fn delete(&self, id: Uuid) -> Result<(), String> {
        let mut index = lock(&self.index);
        let Some(at) = index.results.iter().position(|r| r.id == id) else {
            return Ok(());
        };
        let removed = index.results.remove(at);
        if let Err(e) = self.write(&index) {
            index.results.insert(at, removed);
            return Err(e);
        }
        let _ = std::fs::remove_file(self.file(id)?);
        Ok(())
    }

    /// The rows of the saved result `id`.
    pub fn read(&self, id: Uuid) -> Result<(SchemaRef, Vec<RecordBatch>), String> {
        self.get(id)?;
        read_rows(&self.file(id)?)
    }
}

/// Writes the rows to a Parquet file, which appears whole or not at all; returns their count.
fn write_rows(path: &Path, schema: &Schema, batches: &[RecordBatch]) -> Result<u64, String> {
    let options = ExportOptions {
        format: Format::Parquet,
        ..ExportOptions::default()
    };
    let mut out = Exporter::create(path, schema, &options).map_err(|e| e.to_string())?;
    for batch in batches {
        if let Err(e) = out.write(batch) {
            // The exporter leaves its partial file when it is dropped.
            drop(out);
            let _ = std::fs::remove_file(path.with_extension("parquet.part"));
            return Err(e.to_string());
        }
    }
    out.finish().map_err(|e| e.to_string())
}

/// The rows of a Parquet file written by [`write_rows`], with the metadata of the columns.
fn read_rows(path: &Path) -> Result<(SchemaRef, Vec<RecordBatch>), String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = File::open(path).map_err(|e| at(&e))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .and_then(ParquetRecordBatchReaderBuilder::build)
        .map_err(|e| at(&e))?;
    let schema = reader.schema();
    let batches = reader.collect::<Result<Vec<_>, _>>().map_err(|e| at(&e))?;
    Ok((schema, batches))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use arrow_array::{
        BinaryArray, Decimal128Array, Int32Array, StringArray, TimestampMicrosecondArray,
    };
    use arrow_schema::{DataType, Field, TimeUnit};

    use super::*;

    fn origin(name: &str) -> Origin {
        Origin {
            name: name.to_owned(),
            sql: "SELECT * FROM t".to_owned(),
            connection: "Local".to_owned(),
            driver: "sqlite".to_owned(),
            database: Some("main".to_owned()),
        }
    }

    /// Columns of mixed types, with NULLs, and a database type on the first.
    fn mixed() -> (SchemaRef, Vec<RecordBatch>) {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, true).with_metadata(HashMap::from([(
                META_DB_TYPE.to_owned(),
                "INTEGER".to_owned(),
            )])),
            Field::new("name", DataType::Utf8, true),
            Field::new("price", DataType::Decimal128(10, 2), true),
            Field::new("at", DataType::Timestamp(TimeUnit::Microsecond, None), true),
            Field::new("blob", DataType::Binary, true),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int32Array::from(vec![Some(1), None, Some(3)])),
                Arc::new(StringArray::from(vec![Some("a"), Some("é ü"), None])),
                Arc::new(
                    Decimal128Array::from(vec![Some(12345), None, Some(-5)])
                        .with_precision_and_scale(10, 2)
                        .unwrap(),
                ),
                Arc::new(TimestampMicrosecondArray::from(vec![
                    Some(1_700_000_000_123_456),
                    None,
                    Some(0),
                ])),
                Arc::new(BinaryArray::from_opt_vec(vec![
                    Some(&[0u8, 255, 7][..]),
                    None,
                    Some(&[][..]),
                ])),
            ],
        )
        .unwrap();
        (schema, vec![batch])
    }

    #[test]
    fn rows_of_mixed_types_come_back_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let store = SavedResults::open(Some(dir.path().join("saved-results")));
        let (schema, batches) = mixed();
        let saved = store
            .add(origin("Prices"), &schema, &batches, false)
            .unwrap();
        assert_eq!(saved.rows, 3);
        let (read_schema, read) = store.read(saved.id).unwrap();
        assert_eq!(read_schema.fields(), schema.fields());
        // The database type the grid shows survives in the field's metadata.
        assert_eq!(read_schema.field(0).metadata()[META_DB_TYPE], "INTEGER");
        assert_eq!(read.len(), 1);
        for (i, column) in batches[0].columns().iter().enumerate() {
            assert_eq!(read[0].column(i), column, "column {i}");
        }
    }

    #[test]
    fn the_index_keeps_what_the_list_shows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved-results");
        let store = SavedResults::open(Some(path.clone()));
        assert_eq!(store.list().len(), 0);
        let (schema, batches) = mixed();
        let first = store
            .add(origin(" First "), &schema, &batches, true)
            .unwrap();
        let second = store
            .add(origin("Second"), &schema, &batches, false)
            .unwrap();
        assert_eq!(first.name, "First");
        assert_eq!(first.columns[0].data_type, "INTEGER");
        assert_eq!(first.columns[1].data_type, "Utf8");
        assert!(first.partial);
        // A new store on the same folder reads the same index; the latest comes first.
        let again = SavedResults::open(Some(path));
        let ids: Vec<_> = again.list().iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![second.id, first.id]);
        assert_eq!(again.get(first.id).unwrap(), first);
    }

    #[test]
    fn rename_and_delete_change_the_index_and_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved-results");
        let store = SavedResults::open(Some(path.clone()));
        let (schema, batches) = mixed();
        let saved = store.add(origin("Old"), &schema, &batches, false).unwrap();
        assert_eq!(store.rename(saved.id, "  New ").unwrap().name, "New");
        assert!(store.rename(saved.id, "   ").is_err());
        assert_eq!(SavedResults::open(Some(path.clone())).list()[0].name, "New");
        assert!(store.rename(Uuid::new_v4(), "x").is_err());
        let file = path.join(format!("{}.parquet", saved.id));
        assert!(file.exists());
        store.delete(saved.id).unwrap();
        assert!(!file.exists());
        assert_eq!(store.list().len(), 0);
        assert_eq!(SavedResults::open(Some(path)).list().len(), 0);
        assert!(store.read(saved.id).is_err());
        // Deleting again is not an error.
        store.delete(saved.id).unwrap();
    }

    #[test]
    fn the_index_is_written_whole_and_an_unreadable_one_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved-results");
        let store = SavedResults::open(Some(path.clone()));
        let (schema, batches) = mixed();
        let saved = store.add(origin("A"), &schema, &batches, false).unwrap();
        // Nothing half-written is left behind, and the index parses as JSON.
        let leftovers: Vec<_> = std::fs::read_dir(&path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "tmp" || x == "part"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let text = std::fs::read_to_string(path.join("index.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["results"][0]["id"], saved.id.to_string());
        std::fs::write(path.join("index.json"), "{ not json").unwrap();
        let broken = SavedResults::open(Some(path.clone()));
        assert_eq!(broken.list().len(), 0);
        assert!(path.join("index.json.unreadable").exists());
    }

    #[test]
    fn a_result_without_a_name_or_a_data_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = SavedResults::open(Some(dir.path().join("saved-results")));
        let (schema, batches) = mixed();
        assert!(store.add(origin("  "), &schema, &batches, false).is_err());
        assert_eq!(store.list().len(), 0);
        let nowhere = SavedResults::open(None);
        assert!(nowhere.add(origin("A"), &schema, &batches, false).is_err());
    }

    #[test]
    fn a_result_without_rows_keeps_its_columns() {
        let dir = tempfile::tempdir().unwrap();
        let store = SavedResults::open(Some(dir.path().join("saved-results")));
        let (schema, _) = mixed();
        let saved = store.add(origin("Empty"), &schema, &[], false).unwrap();
        assert_eq!(saved.rows, 0);
        let (read_schema, read) = store.read(saved.id).unwrap();
        assert_eq!(read_schema.fields().len(), 5);
        assert_eq!(read.iter().map(RecordBatch::num_rows).sum::<usize>(), 0);
    }
}
