//! Notes and tags on tables and columns (`platypus_store::CatalogStore`), and the schema
//! documentation written with them.

use std::collections::HashSet;
use std::path::PathBuf;

use futures::StreamExt;
use platypus_core::{Connection, ObjectKind, ObjectMeta, TableDetails};
use platypus_docs::{DocColumn, DocForeignKey, DocIndex, DocKind, DocModel, DocObject, DocSchema};
use platypus_store::{CatalogEntry, CatalogKey, CatalogStore, DisplayColumn, TagCount};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

#[tauri::command]
pub fn catalog_entries(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<CatalogEntry>> {
    state.with_catalog(|c| c.entries(connection_id))
}

#[tauri::command]
pub fn save_catalog_entry(
    state: State<'_, AppState>,
    connection_id: Uuid,
    key: CatalogKey,
    note: String,
    tags: Vec<String>,
) -> CommandResult<CatalogEntry> {
    state.with_catalog(|c| c.save(connection_id, &key, &note, &tags))
}

#[tauri::command]
pub fn catalog_tags(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<TagCount>> {
    state.with_catalog(|c| c.tags(connection_id))
}

/// After the app renamed an object or a column: its notes follow it.
#[tauri::command]
pub fn rename_catalog_key(
    state: State<'_, AppState>,
    connection_id: Uuid,
    from: CatalogKey,
    to: CatalogKey,
) -> CommandResult<()> {
    state.with_catalog(|c| c.rename(connection_id, &from, &to))
}

/// The statements that set the comment of `table`, or of its `column`, as the database
/// writes them; `None` removes it.
#[tauri::command]
pub async fn comment_sql(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
    column: Option<String>,
    comment: Option<String>,
) -> CommandResult<String> {
    let dialect = platypus_sql::Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let old = platypus_schema::load(conn.as_ref(), dialect, &schema, &table).await?;
    let mut new = old.clone();
    let comment = comment.filter(|c| !c.trim().is_empty());
    match &column {
        None => new.comment = comment,
        Some(name) => {
            let c = new
                .columns
                .iter_mut()
                .find(|c| &c.name == name)
                .ok_or_else(|| CommandError::new("not_found", format!("no column {name}")))?;
            c.comment = comment;
        }
    }
    let script = platypus_schema::script(dialect, &schema, Some(&old), &new)
        .map_err(|e| CommandError::new("invalid_design", e))?;
    Ok(script.text(dialect))
}

/// The display columns chosen on a connection, one per table.
#[tauri::command]
pub fn catalog_display_columns(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<DisplayColumn>> {
    state.with_catalog(|c| c.display_columns(connection_id))
}

/// The column that describes the rows of the table `key`; `None` for the automatic choice.
#[tauri::command]
pub fn set_catalog_display_column(
    state: State<'_, AppState>,
    connection_id: Uuid,
    key: CatalogKey,
    column: Option<String>,
) -> CommandResult<()> {
    state.with_catalog(|c| c.set_display_column(connection_id, &key, column.as_deref()))
}

/// Drops the notes of a deleted connection; a catalog that cannot be opened has none.
pub(super) fn forget_connection(catalog: &mut Result<CatalogStore, String>, id: Uuid) {
    if let Ok(c) = catalog {
        let _ = c.remove_connection(id);
    }
}

/// The file `export_docs` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsFormat {
    Markdown,
    Html,
}

/// What to document: the schemas, with their views, diagrams and local notes or not.
#[derive(Debug, Deserialize)]
pub struct DocsRequest {
    pub title: String,
    pub schemas: Vec<String>,
    pub views: bool,
    pub diagrams: bool,
    pub notes: bool,
    pub format: DocsFormat,
    pub path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocsEvent {
    Progress { done: usize, total: usize },
    Finished { path: PathBuf },
    Failed { message: String },
}

/// Documents `request.schemas` on the session's connection and writes the file, at
/// `request.path` or, when that is relative, in the user's documents folder. Returns an id for
/// `cancel_docs`; progress and the outcome arrive on `on_event`, nothing once cancelled.
#[tauri::command]
pub async fn export_docs(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: u64,
    connection_id: Uuid,
    database: Option<String>,
    mut request: DocsRequest,
    on_event: Channel<DocsEvent>,
) -> CommandResult<u64> {
    // The notes are looked up by connection: another connection's would document the wrong one.
    if state.session(session_id)?.connection_id != connection_id {
        return Err(CommandError::new(
            "invalid",
            format!("session {session_id} is not on connection {connection_id}"),
        ));
    }
    let documents = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .ok();
    request.path = docs_path(request.path, documents)?;
    let conn = state.live(session_id).await?;
    // Without its notes the documentation is still worth writing.
    let notes = if request.notes {
        state
            .with_catalog(|c| c.entries(connection_id))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let id = state.next_id();
    let token = CancellationToken::new();
    lock(&state.docs).insert(id, token.clone());
    tauri::async_runtime::spawn(async move {
        let database = database.unwrap_or_default();
        let run = DocsRun {
            conn: conn.as_ref(),
            database: &database,
            request: &request,
            notes: &notes,
            token: &token,
            events: &on_event,
        };
        let outcome = docs_outcome(&run).await;
        lock(&app.state::<AppState>().docs).remove(&id);
        if let Some(event) = outcome {
            let _ = on_event.send(event);
        }
    });
    Ok(id)
}

/// Stops a documentation export; nothing when it has ended.
#[tauri::command]
pub fn cancel_docs(state: State<'_, AppState>, export_id: u64) {
    if let Some(token) = lock(&state.docs).remove(&export_id) {
        token.cancel();
    }
}

/// Where the documentation goes: `path` when it is absolute, else in `base` (the user's
/// documents folder), never in the app's working folder.
fn docs_path(path: PathBuf, base: Option<PathBuf>) -> CommandResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    base.map(|base| base.join(&path)).ok_or_else(|| {
        CommandError::new("invalid", format!("{} is not a full path", path.display()))
    })
}

/// What every object of one documentation export shares.
struct DocsRun<'a> {
    conn: &'a dyn Connection,
    database: &'a str,
    request: &'a DocsRequest,
    notes: &'a [CatalogEntry],
    token: &'a CancellationToken,
    events: &'a Channel<DocsEvent>,
}

/// The objects of a schema that are documented: its tables, without the partitions of a table
/// listed with them (the navigator shows those under their table), and its views when asked.
fn documented(objects: Vec<ObjectMeta>, views: bool) -> Vec<ObjectMeta> {
    let tables: HashSet<String> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(|o| o.name.clone())
        .collect();
    objects
        .into_iter()
        .filter(|o| match o.kind {
            ObjectKind::Table => !o.parent.as_ref().is_some_and(|p| tables.contains(p)),
            ObjectKind::View | ObjectKind::MaterializedView => views,
            _ => false,
        })
        .collect()
}

/// Runs an export to the event that ends it; `None` when it was cancelled. A cancel drops the
/// listing or the reads in flight at once.
async fn docs_outcome(run: &DocsRun<'_>) -> Option<DocsEvent> {
    match run.token.run_until_cancelled(write_docs(run)).await? {
        Ok(false) => None,
        Ok(true) => Some(DocsEvent::Finished {
            path: run.request.path.clone(),
        }),
        Err(message) => Some(DocsEvent::Failed { message }),
    }
}

/// Reads every documented object and writes the file; `Ok(false)` when cancelled before the
/// file is written.
async fn write_docs(run: &DocsRun<'_>) -> Result<bool, String> {
    let request = run.request;
    let mut listed = Vec::with_capacity(request.schemas.len());
    for schema in &request.schemas {
        let objects = run
            .conn
            .objects(schema)
            .await
            .map_err(|e| format!("{schema}: {e}"))?;
        listed.push((schema, documented(objects, request.views)));
    }
    let total = listed.iter().map(|(_, objects)| objects.len()).sum();
    let mut done = 0;
    let mut model = DocModel {
        title: request.title.clone(),
        schemas: Vec::with_capacity(listed.len()),
        diagrams: request.diagrams,
    };
    for (schema, objects) in &listed {
        model
            .schemas
            .push(collect_schema(run, schema, objects, &mut done, total).await);
    }
    let text = match request.format {
        DocsFormat::Markdown => platypus_docs::markdown(&model),
        DocsFormat::Html => platypus_docs::html(&model),
    };
    // Rendering runs without a pause: a cancel during it is seen here.
    if run.token.is_cancelled() {
        return Ok(false);
    }
    tokio::fs::write(&request.path, text)
        .await
        .map_err(|e| format!("cannot write {}: {e}", request.path.display()))?;
    Ok(true)
}

/// The objects of `schema` with their details, read four at a time and counted in `done`.
async fn collect_schema(
    run: &DocsRun<'_>,
    schema: &str,
    objects: &[ObjectMeta],
    done: &mut usize,
    total: usize,
) -> DocSchema {
    // The reads are made first: a closure inside the stream would keep the export's task
    // from being `Send`. They start only once polled.
    let reads: Vec<_> = objects
        .iter()
        .map(|o| run.conn.table_details(schema, &o.name))
        .collect();
    let mut details = futures::stream::iter(reads)
        .buffered(4)
        .zip(futures::stream::iter(objects));
    let mut docs = Vec::with_capacity(objects.len());
    while let Some((read, object)) = details.next().await {
        let read = read.map_err(|e| e.to_string());
        let doc = doc_object(
            run.database,
            schema,
            object,
            read,
            run.notes,
            run.request.notes,
        );
        docs.push(doc);
        *done += 1;
        let _ = run.events.send(DocsEvent::Progress { done: *done, total });
    }
    DocSchema {
        name: schema.to_owned(),
        objects: docs,
    }
}

/// One table or view of the documentation: its details (or why they could not be read) with
/// the connection's notes when `with_notes`.
pub(super) fn doc_object(
    database: &str,
    schema: &str,
    object: &ObjectMeta,
    details: Result<TableDetails, String>,
    notes: &[CatalogEntry],
    with_notes: bool,
) -> DocObject {
    let entry = |column: &str| {
        notes
            .iter()
            .find(|e| {
                with_notes
                    && e.key.database == database
                    && e.key.schema == schema
                    && e.key.object == object.name
                    && e.key.column == column
            })
            .map_or_else(Default::default, |e| (e.note.clone(), e.tags.clone()))
    };
    let (note, tags) = entry("");
    let kind = if object.kind == ObjectKind::Table {
        DocKind::Table
    } else {
        DocKind::View
    };
    let mut doc = DocObject {
        name: object.name.clone(),
        kind,
        comment: object.comment.clone(),
        note,
        tags,
        ..DocObject::default()
    };
    let d = match details {
        Ok(d) => d,
        Err(e) => {
            doc.error = Some(e);
            return doc;
        }
    };
    doc.primary_key = d
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect();
    doc.columns = d
        .columns
        .into_iter()
        .map(|c| {
            let (note, tags) = entry(&c.name);
            DocColumn {
                name: c.name,
                data_type: c.data_type,
                nullable: c.nullable,
                default: c.default,
                primary_key: c.primary_key,
                comment: c.comment,
                note,
                tags,
            }
        })
        .collect();
    doc.foreign_keys = d
        .foreign_keys
        .into_iter()
        .map(|f| DocForeignKey {
            name: f.name,
            columns: f.columns,
            schema: f.ref_schema,
            table: f.ref_table,
            ref_columns: f.ref_columns,
        })
        .collect();
    doc.referenced_by = d
        .referenced_by
        .into_iter()
        .map(|r| DocForeignKey {
            name: r.name,
            columns: r.columns,
            schema: r.schema,
            table: r.table,
            ref_columns: r.ref_columns,
        })
        .collect();
    doc.indexes = d
        .indexes
        .into_iter()
        .filter(|i| !i.primary)
        .map(|i| DocIndex {
            name: i.name,
            columns: i.columns,
            unique: i.unique,
        })
        .collect();
    doc
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use platypus_store::{
        CatalogEntry, CatalogKey, CatalogStore, ConnectionStore, MemorySecrets, PasswordUpdate,
        SavedConnection,
    };
    use uuid::Uuid;

    use crate::commands::AppState;

    fn key() -> CatalogKey {
        CatalogKey {
            object: "t".to_owned(),
            ..CatalogKey::default()
        }
    }

    fn state(
        connections: Result<ConnectionStore, String>,
        catalog: Result<CatalogStore, String>,
    ) -> AppState {
        AppState::new(
            (connections, None),
            Err("no vault".to_owned()),
            Err("no history".to_owned()),
            catalog,
            Err("no settings".to_owned()),
            Err("no scripts".to_owned()),
        )
    }

    #[test]
    fn forget_connection_clears_its_notes() {
        let mut store = Ok(CatalogStore::open_in_memory().unwrap());
        let c = Uuid::new_v4();
        store.as_mut().unwrap().save(c, &key(), "x", &[]).unwrap();
        super::forget_connection(&mut store, c);
        assert_eq!(store.unwrap().entries(c).unwrap(), Vec::new());
    }

    #[test]
    fn forget_connection_keeps_the_notes_of_the_others() {
        let mut store = Ok(CatalogStore::open_in_memory().unwrap());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for c in [a, b] {
            store.as_mut().unwrap().save(c, &key(), "x", &[]).unwrap();
        }
        super::forget_connection(&mut store, a);
        let store = store.unwrap();
        assert_eq!(store.entries(a).unwrap(), Vec::new());
        assert_eq!(store.entries(b).unwrap().len(), 1);
    }

    #[test]
    fn forget_connection_without_a_catalog_does_nothing() {
        let mut store = Err("cannot open".to_owned());
        super::forget_connection(&mut store, Uuid::new_v4());
        assert_eq!(store.unwrap_err(), "cannot open");
    }

    #[test]
    fn a_catalog_that_cannot_be_opened_reports_a_catalog_error() {
        let state = state(
            Err("no connections".to_owned()),
            Err("disk full".to_owned()),
        );
        let err = state
            .with_catalog(|c| c.entries(Uuid::new_v4()))
            .unwrap_err();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "catalog");
        assert_eq!(json["message"], "disk full");
        // Deleting a connection and pruning still go through.
        state.forget_catalog(Uuid::new_v4());
        state.prune_catalog();
    }

    #[test]
    fn deleting_a_connection_forgets_its_notes_through_the_state() {
        let state = state(
            Err("no connections".to_owned()),
            Ok(CatalogStore::open_in_memory().unwrap()),
        );
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for c in [a, b] {
            state.with_catalog(|s| s.save(c, &key(), "x", &[])).unwrap();
        }
        state.forget_catalog(a);
        assert_eq!(state.with_catalog(|s| s.entries(a)).unwrap(), Vec::new());
        assert_eq!(state.with_catalog(|s| s.entries(b)).unwrap().len(), 1);
    }

    #[test]
    fn prune_catalog_keeps_the_saved_connections_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut connections = ConnectionStore::open(
            dir.path().join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        let saved = SavedConnection::new("kept", DriverKind::Postgres);
        let kept = saved.id;
        connections.save(saved, PasswordUpdate::Keep).unwrap();
        let state = state(Ok(connections), Ok(CatalogStore::open_in_memory().unwrap()));
        let gone = Uuid::new_v4();
        for c in [kept, gone] {
            state.with_catalog(|s| s.save(c, &key(), "x", &[])).unwrap();
        }
        state.prune_catalog();
        assert_eq!(state.with_catalog(|s| s.entries(kept)).unwrap().len(), 1);
        assert_eq!(state.with_catalog(|s| s.entries(gone)).unwrap(), Vec::new());
    }

    #[test]
    fn model_takes_notes_and_tags() {
        use platypus_core::{ColumnMeta, ObjectKind, ObjectMeta, TableDetails};
        let object = ObjectMeta::new("orders", ObjectKind::Table, Some("All".to_owned()));
        let details = TableDetails {
            columns: vec![ColumnMeta {
                name: "email".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
                default: None,
                primary_key: false,
                comment: None,
                enum_values: vec![],
                fields: vec![],
            }],
            indexes: vec![],
            foreign_keys: vec![],
            constraints: vec![],
            referenced_by: vec![],
        };
        let key = |column: &str| CatalogKey {
            schema: "public".to_owned(),
            object: "orders".to_owned(),
            column: column.to_owned(),
            ..CatalogKey::default()
        };
        let notes = vec![
            CatalogEntry {
                key: key(""),
                note: "per order".to_owned(),
                tags: vec![],
            },
            CatalogEntry {
                key: key("email"),
                note: String::new(),
                tags: vec!["pii".to_owned()],
            },
        ];
        let doc = super::doc_object("", "public", &object, Ok(details), &notes, true);
        assert_eq!(doc.comment.as_deref(), Some("All"));
        assert_eq!(doc.note, "per order");
        assert_eq!(doc.columns[0].tags, ["pii"]);
    }

    #[test]
    fn model_without_notes_or_details() {
        use platypus_core::{ObjectKind, ObjectMeta};
        let object = ObjectMeta::new("v", ObjectKind::View, None);
        let notes = vec![CatalogEntry {
            key: CatalogKey {
                object: "v".to_owned(),
                ..CatalogKey::default()
            },
            note: "hidden".to_owned(),
            tags: vec![],
        }];
        let doc = super::doc_object("", "", &object, Err("denied".to_owned()), &notes, false);
        assert_eq!(doc.kind, platypus_docs::DocKind::View);
        assert_eq!(doc.note, "");
        assert_eq!(doc.error.as_deref(), Some("denied"));
    }

    #[test]
    fn documents_tables_and_views_when_asked_without_partitions() {
        use platypus_core::{ObjectKind, ObjectMeta};
        let object = |name: &str, kind, parent: Option<&str>| ObjectMeta {
            parent: parent.map(str::to_owned),
            ..ObjectMeta::new(name, kind, None)
        };
        let objects = vec![
            object("orders", ObjectKind::Table, None),
            object("orders_2026", ObjectKind::Table, Some("orders")),
            object("lone_part", ObjectKind::Table, Some("elsewhere")),
            object("v", ObjectKind::View, None),
            object("mv", ObjectKind::MaterializedView, None),
            object("s", ObjectKind::Sequence, None),
            object("trg", ObjectKind::Trigger, Some("orders")),
        ];
        let names = |views| -> Vec<String> {
            super::documented(objects.clone(), views)
                .into_iter()
                .map(|o| o.name)
                .collect()
        };
        assert_eq!(names(false), ["orders", "lone_part"]);
        assert_eq!(names(true), ["orders", "lone_part", "v", "mv"]);
    }

    #[test]
    fn a_relative_path_goes_in_the_documents_folder() {
        let documents = std::env::temp_dir();
        let absolute = documents.join("docs.md");
        assert_eq!(super::docs_path(absolute.clone(), None).unwrap(), absolute);
        assert_eq!(
            super::docs_path("docs.md".into(), Some(documents.clone())).unwrap(),
            absolute
        );
        let err = super::docs_path("docs.md".into(), None).unwrap_err();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "invalid");
        assert_eq!(json["message"], "docs.md is not a full path");
    }

    mod export {
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, ExecOptions};
        use serde_json::{Value, json};
        use tauri::ipc::{Channel, InvokeResponseBody};
        use tokio_util::sync::CancellationToken;

        use super::super::{DocsEvent, DocsFormat, DocsRequest, DocsRun, docs_outcome};

        /// A SQLite database with two tables and a view.
        async fn database(dir: &Path) -> Box<dyn Connection> {
            let path = dir.join("shop.db");
            std::fs::File::create(&path).unwrap();
            let mut config = ConnectionConfig::new(DriverKind::Sqlite);
            config.database = path.to_string_lossy().into_owned();
            let conn = platypus_driver_sqlite::SqliteDriver
                .connect(&config)
                .await
                .unwrap();
            for sql in [
                "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT)",
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id))",
                "CREATE VIEW recent AS SELECT id FROM orders",
            ] {
                conn.execute(sql, &ExecOptions::default()).await.unwrap();
            }
            conn
        }

        fn request(path: PathBuf, format: DocsFormat, views: bool) -> DocsRequest {
            DocsRequest {
                title: "shop".to_owned(),
                schemas: vec!["main".to_owned()],
                views,
                diagrams: true,
                notes: false,
                format,
                path,
            }
        }

        /// Runs an export: the event that ends it, and the progress events sent before.
        fn export(request: &DocsRequest, token: &CancellationToken) -> (Option<Value>, Vec<Value>) {
            let dir = tempfile::tempdir().unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&seen);
            let events = Channel::<DocsEvent>::new(move |body| {
                if let InvokeResponseBody::Json(json) = body {
                    sink.lock()
                        .unwrap()
                        .push(serde_json::from_str(&json).unwrap());
                }
                Ok(())
            });
            let outcome = tauri::async_runtime::block_on(async {
                let conn = database(dir.path()).await;
                let run = DocsRun {
                    conn: conn.as_ref(),
                    database: "",
                    request,
                    notes: &[],
                    token,
                    events: &events,
                };
                docs_outcome(&run).await
            });
            let outcome = outcome.map(|e| serde_json::to_value(e).unwrap());
            let seen = seen.lock().unwrap().clone();
            (outcome, seen)
        }

        #[test]
        fn writes_the_file_after_one_progress_event_per_object() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("shop.html");
            let request = request(path.clone(), DocsFormat::Html, false);
            let (outcome, progress) = export(&request, &CancellationToken::new());
            assert_eq!(outcome, Some(json!({ "kind": "finished", "path": path })));
            assert_eq!(
                progress,
                [
                    json!({ "kind": "progress", "done": 1, "total": 2 }),
                    json!({ "kind": "progress", "done": 2, "total": 2 }),
                ]
            );
            let page = std::fs::read_to_string(&path).unwrap();
            assert!(
                page.contains("<pre class=\"mermaid\">erDiagram\n"),
                "{page}"
            );
            assert!(page.contains("main.orders"), "{page}");
            assert!(!page.contains("main.recent"), "{page}");
        }

        #[test]
        fn views_are_documented_when_asked() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("shop.md");
            let request = request(path.clone(), DocsFormat::Markdown, true);
            let (outcome, progress) = export(&request, &CancellationToken::new());
            assert_eq!(outcome.unwrap()["kind"], "finished");
            assert_eq!(progress.len(), 3);
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains("## main.recent"), "{text}");
            assert!(text.contains("```mermaid\nerDiagram\n"), "{text}");
        }

        #[test]
        fn a_cancelled_export_writes_nothing() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("shop.md");
            let token = CancellationToken::new();
            token.cancel();
            let (outcome, progress) =
                export(&request(path.clone(), DocsFormat::Markdown, true), &token);
            assert_eq!(outcome, None);
            assert_eq!(progress, Vec::<Value>::new());
            assert!(!path.exists());
        }

        #[test]
        fn a_file_that_cannot_be_written_fails() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("missing").join("shop.md");
            let (outcome, progress) = export(
                &request(path, DocsFormat::Markdown, false),
                &CancellationToken::new(),
            );
            let outcome = outcome.unwrap();
            assert_eq!(outcome["kind"], "failed");
            let message = outcome["message"].as_str().unwrap();
            assert!(message.starts_with("cannot write "), "{message}");
            assert_eq!(progress.len(), 2);
        }
    }
}
