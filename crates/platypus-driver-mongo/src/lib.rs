//! MongoDB driver on the official `mongodb` crate (rustls, no OpenSSL).
//!
//! Statements are mongosh, not SQL: [`shell`] parses the subset the editor runs. Results
//! become Arrow batches with one column per top-level field ([`docs`]). Schemas are
//! databases, objects are collections and views. There are no transactions.

mod config;
pub mod docs;
mod error;
pub mod generate;
pub mod literal;
mod meta;
pub mod shell;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::AggregateOptions;
use mongodb::{Client, Cursor, Database};
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, Connection, ConnectionConfig, Driver,
    DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, ObjectRef, Property, RecordBatchStream,
    ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, unsupported,
};
use tokio::sync::watch;

use crate::error::{map_error, parse_error};
use crate::shell::{CollectionOp, Command, Find};

/// Numbers the sessions of the process, for their operation comments.
static SESSIONS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default)]
pub struct MongoDriver;

#[async_trait]
impl Driver for MongoDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Mongo
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let target = config::client_options(config).await?;
        let client =
            Client::with_options(target.options).map_err(|e| Error::Connect(e.to_string()))?;
        let admin = client.database("admin");
        let build = admin
            .run_command(doc! { "buildInfo": 1 })
            .await
            .map_err(|e| Error::Connect(error::message(&e)))?;
        let version = build.get_str("version").unwrap_or_default().to_owned();
        let (cancel, _) = watch::channel(0u64);
        let conn = MongoConnection {
            client,
            database: Mutex::new(target.database),
            info: ServerInfo {
                product: "MongoDB".to_owned(),
                version,
                encrypted: target.encrypted,
            },
            closed: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(cancel),
            comment: format!(
                "platypus:{}:{}",
                std::process::id(),
                SESSIONS.fetch_add(1, Ordering::Relaxed)
            ),
        };
        for statement in config.init.sql(DriverKind::Mongo)? {
            conn.execute(&statement, &ExecOptions::default()).await?;
        }
        Ok(Box::new(conn))
    }
}

/// A MongoDB client: a pool of connections that reconnects by itself, plus the current
/// database that `use` changes.
#[derive(Debug)]
pub struct MongoConnection {
    client: Client,
    database: Mutex<String>,
    info: ServerInfo,
    /// The last operation failed because the server could not be reached.
    closed: Arc<AtomicBool>,
    /// Bumped by the cancel handle; running statements and streams watch it.
    cancel: Arc<watch::Sender<u64>>,
    /// Sent with every operation, so the cancel handle can find them in `$currentOp`.
    comment: String,
}

/// Waits until the cancel handle is used after the subscription.
async fn cancelled(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

impl MongoConnection {
    fn current_database(&self) -> String {
        self.database
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn db(&self, name: Option<&str>) -> Database {
        match name {
            Some(name) => self.client.database(name),
            None => self.client.database(&self.current_database()),
        }
    }

    /// Records whether the server was reachable, for [`Connection::is_closed`].
    fn track<T>(&self, result: mongodb::error::Result<T>) -> Result<T> {
        match result {
            Ok(v) => {
                self.closed.store(false, Ordering::Relaxed);
                Ok(v)
            }
            Err(e) => {
                if error::is_network(&e) {
                    self.closed.store(true, Ordering::Relaxed);
                }
                Err(map_error(&e))
            }
        }
    }

    async fn run(&self, command: Command, options: &ExecOptions) -> Result<ExecResult> {
        let batch = u32::try_from(options.batch_size.max(1)).unwrap_or(u32::MAX);
        match command {
            Command::ShowDatabases => {
                let reply = self.track(
                    self.client
                        .database("admin")
                        .run_command(doc! { "listDatabases": 1 })
                        .await,
                )?;
                let docs = documents(reply.get_array("databases").ok());
                docs_result(&docs, &["name", "sizeOnDisk", "empty"])
            }
            Command::ShowCollections => {
                let names = self.track(self.db(None).list_collection_names().await)?;
                names_result(names)
            }
            Command::Use(name) => {
                *self.database.lock().unwrap_or_else(PoisonError::into_inner) = name;
                Ok(ExecResult::Affected { rows: 0 })
            }
            Command::ListCollections {
                db,
                filter,
                names_only: true,
            } => {
                let names = self.track(
                    self.db(db.as_deref())
                        .list_collection_names()
                        .filter(filter)
                        .await,
                )?;
                names_result(names)
            }
            Command::ListCollections { db, filter, .. } => {
                let cursor = self.track(
                    self.db(db.as_deref())
                        .run_cursor_command(doc! { "listCollections": 1, "filter": filter })
                        .batch_size(batch)
                        .await,
                )?;
                self.stream(cursor, options).await
            }
            Command::RunCommand { db, command } => {
                let reply = self.track(self.db(db.as_deref()).run_command(command).await)?;
                // A command that opens a cursor (`find`, `aggregate`, …): its first batch.
                let first_batch = reply
                    .get_document("cursor")
                    .ok()
                    .and_then(|c| c.get_array("firstBatch").ok());
                match first_batch {
                    Some(items) => docs_result(&documents(Some(items)), &[]),
                    None => docs_result(&[reply], &[]),
                }
            }
            Command::CreateCollection { db, name, options } => {
                let mut command = doc! { "create": name };
                command.extend(options);
                self.track(self.db(db.as_deref()).run_command(command).await)?;
                Ok(ExecResult::Affected { rows: 0 })
            }
            Command::Collection { db, collection, op } => {
                self.collection_op(db.as_deref(), &collection, op, batch, options)
                    .await
            }
        }
    }

    async fn collection_op(
        &self,
        db: Option<&str>,
        name: &str,
        op: CollectionOp,
        batch: u32,
        options: &ExecOptions,
    ) -> Result<ExecResult> {
        let database = self.db(db);
        let coll = database.collection::<Document>(name);
        let comment = Bson::String(self.comment.clone());
        match op {
            CollectionOp::Find(find) => {
                let cursor = self.track(self.find(&coll, find, batch).await)?;
                self.stream(cursor, options).await
            }
            CollectionOp::Aggregate {
                pipeline,
                options: aggregate,
            } => {
                let mut opts: AggregateOptions = mongodb::bson::from_document(aggregate)
                    .map_err(|e| Error::Other(format!("invalid aggregate options: {e}")))?;
                opts.batch_size = Some(batch);
                opts.comment = Some(comment);
                let cursor = self.track(coll.aggregate(pipeline).with_options(opts).await)?;
                self.stream(cursor, options).await
            }
            CollectionOp::CountDocuments {
                filter,
                skip,
                limit,
            } => {
                let mut count = coll.count_documents(filter).comment(comment);
                if let Some(skip) = skip {
                    count = count.skip(skip);
                }
                if let Some(limit) = limit {
                    count = count.limit(limit);
                }
                let n = self.track(count.await)?;
                docs_result(
                    &[doc! { "count": i64::try_from(n).unwrap_or(i64::MAX) }],
                    &[],
                )
            }
            CollectionOp::EstimatedDocumentCount => {
                let n = self.track(coll.estimated_document_count().comment(comment).await)?;
                docs_result(
                    &[doc! { "count": i64::try_from(n).unwrap_or(i64::MAX) }],
                    &[],
                )
            }
            CollectionOp::Distinct { field, filter } => {
                let values = self.track(coll.distinct(&field, filter).comment(comment).await)?;
                let schema = docs::value_schema(&field, &values);
                let rows: Vec<Document> = values.into_iter().map(|v| doc! { &field: v }).collect();
                let batch = docs::to_batch(&schema, &rows)?;
                Ok(rows_of(schema, vec![batch]))
            }
            op @ (CollectionOp::InsertOne(_)
            | CollectionOp::InsertMany(_)
            | CollectionOp::Update { .. }
            | CollectionOp::Replace { .. }
            | CollectionOp::Delete { .. }
            | CollectionOp::Drop
            | CollectionOp::CreateIndex { .. }
            | CollectionOp::DropIndex(_)
            | CollectionOp::Rename { .. }) => self.write_op(&coll, op).await.map(affected_rows),
            CollectionOp::GetIndexes => {
                let cursor = self.track(
                    database
                        .run_cursor_command(doc! { "listIndexes": name })
                        .batch_size(batch)
                        .await,
                )?;
                self.stream(cursor, options).await
            }
        }
    }

    /// The documents a write changed (0 for changes to collections and indexes).
    async fn write_op(
        &self,
        coll: &mongodb::Collection<Document>,
        op: CollectionOp,
    ) -> Result<u64> {
        match op {
            CollectionOp::InsertOne(document) => {
                self.track(coll.insert_one(document).await)?;
                Ok(1)
            }
            CollectionOp::InsertMany(documents) => {
                if documents.is_empty() {
                    return Ok(0);
                }
                let result = self.track(coll.insert_many(documents).await)?;
                Ok(result.inserted_ids.len() as u64)
            }
            CollectionOp::Update {
                filter,
                update,
                many,
                upsert,
            } => {
                let update = match update {
                    Bson::Array(stages) => {
                        mongodb::options::UpdateModifications::Pipeline(documents(Some(&stages)))
                    }
                    Bson::Document(d) => mongodb::options::UpdateModifications::Document(d),
                    _ => return Err(Error::Other("invalid update".to_owned())),
                };
                let result = if many {
                    self.track(coll.update_many(filter, update).upsert(upsert).await)?
                } else {
                    self.track(coll.update_one(filter, update).upsert(upsert).await)?
                };
                Ok(result.matched_count + u64::from(result.upserted_id.is_some()))
            }
            CollectionOp::Replace {
                filter,
                replacement,
                upsert,
            } => {
                let result =
                    self.track(coll.replace_one(filter, replacement).upsert(upsert).await)?;
                Ok(result.matched_count + u64::from(result.upserted_id.is_some()))
            }
            CollectionOp::Delete { filter, many } => {
                let result = if many {
                    self.track(coll.delete_many(filter).await)?
                } else {
                    self.track(coll.delete_one(filter).await)?
                };
                Ok(result.deleted_count)
            }
            CollectionOp::Drop => {
                self.track(coll.drop().await)?;
                Ok(0)
            }
            CollectionOp::CreateIndex {
                keys,
                options: index,
            } => {
                let mut spec = doc! { "key": keys.clone(), "name": meta::index_name(&keys) };
                spec.extend(index);
                self.track(
                    self.client
                        .database(&coll.namespace().db)
                        .run_command(doc! { "createIndexes": coll.name(), "indexes": [spec] })
                        .await,
                )?;
                Ok(0)
            }
            CollectionOp::DropIndex(index) => {
                self.track(
                    self.client
                        .database(&coll.namespace().db)
                        .run_command(doc! { "dropIndexes": coll.name(), "index": index })
                        .await,
                )?;
                Ok(0)
            }
            CollectionOp::Rename { to, drop_target } => {
                let from = coll.namespace().to_string();
                let to = format!("{}.{to}", coll.namespace().db);
                self.track(
                    self.client
                        .database("admin")
                        .run_command(
                            doc! { "renameCollection": from, "to": to, "dropTarget": drop_target },
                        )
                        .await,
                )?;
                Ok(0)
            }
            _ => unreachable!("not a write"),
        }
    }

    async fn find(
        &self,
        coll: &mongodb::Collection<Document>,
        find: Find,
        batch: u32,
    ) -> mongodb::error::Result<Cursor<Document>> {
        let mut action = coll
            .find(find.filter)
            .batch_size(batch)
            .comment(Bson::String(self.comment.clone()));
        if let Some(projection) = find.projection {
            action = action.projection(projection);
        }
        if let Some(sort) = find.sort {
            action = action.sort(sort);
        }
        if let Some(skip) = find.skip {
            action = action.skip(skip);
        }
        if let Some(limit) = find.limit {
            action = action.limit(limit);
        }
        action.await
    }

    /// The documents of `cursor` as a stream of batches. The first batch is read here, so
    /// its fields make the schema; fields that appear only later are left out.
    async fn stream(
        &self,
        mut cursor: Cursor<Document>,
        options: &ExecOptions,
    ) -> Result<ExecResult> {
        let size = options.batch_size.max(1);
        let mut rx = self.cancel.subscribe();
        let first = next_batch(&mut cursor, size, &mut rx, &self.closed).await?;
        let schema = docs::infer_schema(&first);
        let first_batch = docs::to_batch(&schema, &first)?;
        let done = first.len() < size;
        let closed = Arc::clone(&self.closed);
        let state = (Some(first_batch), (!done).then_some(cursor), rx);
        let rest_schema = Arc::clone(&schema);
        let inner: BoxStream<'static, Result<arrow_array::RecordBatch>> =
            stream::unfold(state, move |(first, cursor, mut rx)| {
                let schema = Arc::clone(&rest_schema);
                let closed = Arc::clone(&closed);
                async move {
                    if let Some(first) = first {
                        return Some((Ok(first), (None, cursor, rx)));
                    }
                    let mut cursor = cursor?;
                    match next_batch(&mut cursor, size, &mut rx, &closed).await {
                        Ok(docs) if docs.is_empty() => None,
                        Ok(docs) => {
                            let more = docs.len() == size;
                            let batch = docs::to_batch(&schema, &docs);
                            Some((batch, (None, more.then_some(cursor), rx)))
                        }
                        Err(e) => Some((Err(e), (None, None, rx))),
                    }
                }
            })
            .filter(|b| std::future::ready(!matches!(b, Ok(b) if b.num_rows() == 0)))
            .boxed();
        Ok(ExecResult::Rows(RecordBatchStream::new(schema, inner)))
    }
}

/// Up to `size` documents of `cursor`; fails with [`Error::Cancelled`] when the cancel
/// handle is used meanwhile.
async fn next_batch(
    cursor: &mut Cursor<Document>,
    size: usize,
    rx: &mut watch::Receiver<u64>,
    closed: &AtomicBool,
) -> Result<Vec<Document>> {
    let mut out = Vec::with_capacity(size.min(1024));
    while out.len() < size {
        tokio::select! {
            next = cursor.next() => match next {
                Some(Ok(doc)) => out.push(doc),
                Some(Err(e)) => {
                    if error::is_network(&e) {
                        closed.store(true, Ordering::Relaxed);
                    }
                    return Err(map_error(&e));
                }
                None => break,
            },
            () = cancelled(rx) => return Err(Error::Cancelled),
        }
    }
    Ok(out)
}

fn affected_rows(rows: u64) -> ExecResult {
    ExecResult::Affected { rows }
}

fn documents(items: Option<&Vec<Bson>>) -> Vec<Document> {
    items
        .into_iter()
        .flatten()
        .filter_map(|b| b.as_document().cloned())
        .collect()
}

fn rows_of(schema: arrow_schema::SchemaRef, batches: Vec<arrow_array::RecordBatch>) -> ExecResult {
    ExecResult::Rows(RecordBatchStream::new(
        schema,
        stream::iter(batches.into_iter().map(Ok)).boxed(),
    ))
}

/// Rows of a few documents read at once; `fields` keeps only those fields when not empty.
fn docs_result(docs: &[Document], fields: &[&str]) -> Result<ExecResult> {
    let docs: Vec<Document> = if fields.is_empty() {
        docs.to_vec()
    } else {
        docs.iter()
            .map(|d| {
                fields
                    .iter()
                    .filter_map(|&f| d.get(f).map(|v| (f.to_owned(), v.clone())))
                    .collect()
            })
            .collect()
    };
    let schema = docs::infer_schema(&docs);
    let batch = docs::to_batch(&schema, &docs)?;
    Ok(rows_of(schema, vec![batch]))
}

fn names_result(mut names: Vec<String>) -> Result<ExecResult> {
    names.sort();
    let docs: Vec<Document> = names.into_iter().map(|n| doc! { "name": n }).collect();
    let schema = docs::value_schema("name", &[Bson::String(String::new())]);
    let batch = docs::to_batch(&schema, &docs)?;
    Ok(rows_of(schema, vec![batch]))
}

struct MongoCanceller {
    client: Client,
    cancel: Arc<watch::Sender<u64>>,
    comment: String,
}

#[async_trait]
impl Canceller for MongoCanceller {
    async fn cancel(&self) -> Result<()> {
        self.cancel.send_modify(|n| *n += 1);
        // Stop the operations on the server too: dropping their futures only closes the
        // socket, which the server notices when it next writes.
        let admin = self.client.database("admin");
        let ops = admin
            .aggregate(vec![
                doc! { "$currentOp": {} },
                doc! { "$match": { "command.comment": &self.comment } },
            ])
            .await;
        if let Ok(mut ops) = ops {
            while let Some(Ok(op)) = ops.next().await {
                if let Some(id) = op.get("opid") {
                    let _ = admin
                        .run_command(doc! { "killOp": 1, "op": id.clone() })
                        .await;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Connection for MongoConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let command = shell::parse(sql).map_err(|e| parse_error(sql, &e))?;
        let mut rx = self.cancel.subscribe();
        tokio::select! {
            result = self.run(command, options) => result,
            () = cancelled(&mut rx) => Err(Error::Cancelled),
        }
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(MongoCanceller {
            client: self.client.clone(),
            cancel: Arc::clone(&self.cancel),
            comment: self.comment.clone(),
        })
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        let names = self.track(self.client.list_database_names().await)?;
        let current = self.current_database();
        Ok(meta::schemas(names, &current))
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        meta::objects(self, schema).await
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        meta::table_details(self, schema, table).await
    }

    async fn referencing_keys(&self, _schema: &str, _table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        meta::catalog_columns(self).await
    }

    fn transaction(&self) -> TransactionState {
        TransactionState {
            autocommit: true,
            open: false,
        }
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        if on {
            Ok(())
        } else {
            Err(unsupported("transactions on MongoDB"))
        }
    }

    async fn commit(&self) -> Result<()> {
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: false,
            cancel: true,
            explain: false,
            object_ddl: true,
            multiple_databases: false,
            metadata_search: false,
        }
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(self, object).await
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        meta::object_properties(self, object).await
    }
}
