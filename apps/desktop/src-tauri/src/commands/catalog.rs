//! Notes and tags on tables and columns (`platypus_store::CatalogStore`), and the schema
//! documentation written with them.

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
    Ok(script.text())
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

/// Documents `request.schemas` on the session's connection and writes the file. Returns an
/// id for `cancel_docs`; progress and the outcome arrive on `on_event`, nothing once cancelled.
#[tauri::command]
pub async fn export_docs(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: u64,
    connection_id: Uuid,
    database: Option<String>,
    request: DocsRequest,
    on_event: Channel<DocsEvent>,
) -> CommandResult<u64> {
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
        let outcome = write_docs(&run).await;
        lock(&app.state::<AppState>().docs).remove(&id);
        let event = match outcome {
            Ok(false) => return,
            Ok(true) => DocsEvent::Finished {
                path: request.path.clone(),
            },
            Err(message) => DocsEvent::Failed { message },
        };
        let _ = on_event.send(event);
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

/// What every object of one documentation export shares.
struct DocsRun<'a> {
    conn: &'a dyn Connection,
    database: &'a str,
    request: &'a DocsRequest,
    notes: &'a [CatalogEntry],
    token: &'a CancellationToken,
    events: &'a Channel<DocsEvent>,
}

/// Whether objects of `kind` are documented: tables, and views when asked.
fn documented(kind: ObjectKind, views: bool) -> bool {
    kind == ObjectKind::Table
        || (views && matches!(kind, ObjectKind::View | ObjectKind::MaterializedView))
}

/// Reads every documented object and writes the file; `Ok(false)` when cancelled first.
async fn write_docs(run: &DocsRun<'_>) -> Result<bool, String> {
    let request = run.request;
    let mut listed = Vec::with_capacity(request.schemas.len());
    for schema in &request.schemas {
        let objects = run
            .conn
            .objects(schema)
            .await
            .map_err(|e| format!("{schema}: {e}"))?;
        let kept: Vec<ObjectMeta> = objects
            .into_iter()
            .filter(|o| documented(o.kind, request.views))
            .collect();
        listed.push((schema, kept));
    }
    let total = listed.iter().map(|(_, objects)| objects.len()).sum();
    let mut done = 0;
    let mut model = DocModel {
        title: request.title.clone(),
        schemas: Vec::with_capacity(listed.len()),
        diagrams: request.diagrams,
    };
    for (schema, objects) in &listed {
        let Some(doc) = collect_schema(run, schema, objects, &mut done, total).await else {
            return Ok(false);
        };
        model.schemas.push(doc);
    }
    let text = match request.format {
        DocsFormat::Markdown => platypus_docs::markdown(&model),
        DocsFormat::Html => platypus_docs::html(&model),
    };
    if run.token.is_cancelled() {
        return Ok(false);
    }
    tokio::fs::write(&request.path, text)
        .await
        .map_err(|e| format!("cannot write {}: {e}", request.path.display()))?;
    Ok(true)
}

/// The objects of `schema` with their details, read four at a time and counted in `done`;
/// `None` once cancelled.
async fn collect_schema(
    run: &DocsRun<'_>,
    schema: &str,
    objects: &[ObjectMeta],
    done: &mut usize,
    total: usize,
) -> Option<DocSchema> {
    // The reads are made first: a closure inside the stream would keep the export's task
    // from being `Send`. They start only once polled.
    let reads: Vec<_> = objects
        .iter()
        .map(|o| run.conn.table_details(schema, &o.name))
        .collect();
    let mut details = futures::stream::iter(reads).buffered(4);
    let mut docs = Vec::with_capacity(objects.len());
    for object in objects {
        let read = details.next().await?;
        if run.token.is_cancelled() {
            return None;
        }
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
    Some(DocSchema {
        name: schema.to_owned(),
        objects: docs,
    })
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
        assert!(super::documented(ObjectKind::Table, false));
        assert!(!super::documented(ObjectKind::View, false));
        assert!(super::documented(ObjectKind::MaterializedView, true));
        assert!(!super::documented(ObjectKind::Sequence, true));
    }
}
