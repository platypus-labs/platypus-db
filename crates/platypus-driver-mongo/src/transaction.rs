//! Manual commit: one [`ClientSession`] for the connection, a transaction opened in it at the
//! first statement that runs in one, and what MongoDB refuses to run there.
//!
//! Only replica sets and sharded clusters run transactions. The server ends a transaction by
//! itself when it stays open longer than `transactionLifetimeLimitSeconds` (60 by default),
//! and on some errors: the driver learns it from the next answer, or asks after an error.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use mongodb::bson::{Bson, Document, doc};
use mongodb::error::{ErrorKind, TRANSIENT_TRANSACTION_ERROR, UNKNOWN_TRANSACTION_COMMIT_RESULT};
use mongodb::options::{ReadPreference, SelectionCriteria};
use mongodb::{ClientSession, Database};
use platypus_core::{Error, Result, TransactionState, TxTracker};

use crate::error::{is_network, map_error};
use crate::shell::{CollectionOp, Command};

/// Where a statement runs in manual mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// In the transaction: reads and writes of documents, and the creations MongoDB takes in one.
    Transaction,
    /// On its own, as in autocommit mode: metadata, server information, administration. The
    /// transaction does not see it, and it does not touch the transaction.
    Outside,
    /// Refused: MongoDB does not run it in a transaction, and run outside it would apply at once,
    /// which a rollback could not undo. The name of what is refused.
    Refused(&'static str),
}

/// The databases whose collections a transaction cannot read or write.
const SYSTEM_DATABASES: [&str; 3] = ["admin", "config", "local"];

/// The aggregation stages that read the server's metadata, which a transaction refuses.
const METADATA_STAGES: [&str; 8] = [
    "$collStats",
    "$currentOp",
    "$indexStats",
    "$listLocalSessions",
    "$listSessions",
    "$planCacheStats",
    "$listSearchIndexes",
    "$changeStream",
];

/// Where `command` runs in manual mode; `current` is the session's database, for a command
/// that names none.
#[must_use]
pub fn placement(command: &Command, current: &str) -> Placement {
    match command {
        Command::ShowDatabases
        | Command::ShowCollections
        | Command::Use(_)
        | Command::DatabaseName { .. }
        | Command::ListCollections { .. } => Placement::Outside,
        Command::RunCommand { db, command } => {
            command_placement(command, db.as_deref().unwrap_or(current))
        }
        Command::CreateCollection { db, options, .. } => {
            create_placement(db.as_deref().unwrap_or(current), options)
        }
        Command::Collection { db, collection, op } => in_reach(
            op_placement(op),
            db.as_deref().unwrap_or(current),
            collection,
            crate::shell::is_read_only(command),
        ),
    }
}

/// What writes to a database or collection a transaction cannot reach.
const SYSTEM_WRITE: &str =
    "writing to the admin, config and local databases or to system collections";

/// `place` for an operation on collection `collection` of database `db`: what would run in the
/// transaction there runs outside it when it `reads`, and is refused when it writes.
fn in_reach(place: Placement, db: &str, collection: &str, reads: bool) -> Placement {
    match place {
        Placement::Transaction if system(db, collection) && reads => Placement::Outside,
        Placement::Transaction if system(db, collection) => Placement::Refused(SYSTEM_WRITE),
        place => place,
    }
}

/// `create` in database `db`, with `options`.
fn create_placement(db: &str, options: &Document) -> Placement {
    if system(db, "") {
        Placement::Refused(SYSTEM_WRITE)
    } else if options.contains_key("viewOn") || options.contains_key("timeseries") {
        Placement::Refused("creating a view or a time-series collection")
    } else {
        Placement::Transaction
    }
}

/// A server command (`db.runCommand(…)`, `db.adminCommand(…)`, `db.aggregate(…)`,
/// `db.dropDatabase()` and the other helpers that send one) in database `db`, by its name, as
/// the collection methods that send the same command: the writes of documents run in the
/// transaction, so that none of them is applied at once; the changes to collections and
/// databases, which a transaction does not take, are refused; the rest (server information,
/// administration) runs outside.
fn command_placement(command: &Document, db: &str) -> Placement {
    let Some((name, target)) = command.iter().next() else {
        return Placement::Outside;
    };
    // `{ aggregate: 1 }` and the like name no collection.
    let collection = target.as_str().unwrap_or_default();
    let reach = |place: Placement, reads: bool| in_reach(place, db, collection, reads);
    match name.to_ascii_lowercase().as_str() {
        "find" | "distinct" => reach(Placement::Transaction, true),
        "aggregate" => {
            let stages: Vec<&Document> = command
                .get_array("pipeline")
                .map(|stages| stages.iter().filter_map(Bson::as_document).collect())
                .unwrap_or_default();
            reach(pipeline_placement(&stages), true)
        }
        "insert" | "update" | "delete" | "findandmodify" | "createindexes" => {
            reach(Placement::Transaction, false)
        }
        "create" => create_placement(db, command),
        "drop" => Placement::Refused("dropping a collection"),
        "dropdatabase" => Placement::Refused("dropping a database"),
        "renamecollection" => Placement::Refused("renaming a collection"),
        "dropindexes" | "deleteindexes" => Placement::Refused("dropping an index"),
        "collmod"
        | "converttocapped"
        | "clonecollectionascapped"
        | "reindex"
        | "compact"
        | "emptycapped"
        | "createsearchindexes"
        | "updatesearchindex"
        | "dropsearchindex"
        | "shardcollection"
        | "reshardcollection"
        | "refinecollectionshardkey"
        | "unshardcollection"
        | "movecollection" => Placement::Refused("changing a collection"),
        "mapreduce" | "applyops" | "bulkwrite" => Placement::Refused("this command"),
        _ => Placement::Outside,
    }
}

/// Whether a transaction cannot reach collection `collection` of database `db`.
fn system(db: &str, collection: &str) -> bool {
    SYSTEM_DATABASES.contains(&db) || collection.starts_with("system.")
}

/// Whether the documents of collection `collection` of database `db` can be written in a
/// transaction.
#[must_use]
pub fn reachable(db: &str, collection: &str) -> bool {
    !system(db, collection)
}

/// An aggregation with `stages`.
fn pipeline_placement(stages: &[&Document]) -> Placement {
    let has = |names: &[&str]| {
        stages
            .iter()
            .any(|s| names.iter().any(|name| s.contains_key(name)))
    };
    if has(&["$out", "$merge"]) {
        Placement::Refused("an aggregation that writes with $out or $merge")
    } else if has(&METADATA_STAGES) {
        Placement::Outside
    } else {
        Placement::Transaction
    }
}

fn op_placement(op: &CollectionOp) -> Placement {
    match op {
        CollectionOp::Find(_)
        | CollectionOp::CountDocuments { .. }
        | CollectionOp::Distinct { .. }
        | CollectionOp::InsertOne(_)
        | CollectionOp::InsertMany(_)
        | CollectionOp::InsertWith { .. }
        | CollectionOp::Update { .. }
        | CollectionOp::Replace { .. }
        | CollectionOp::Delete { .. }
        | CollectionOp::BulkWrite { .. }
        | CollectionOp::FindOneAndUpdate { .. }
        | CollectionOp::FindOneAndReplace { .. }
        | CollectionOp::FindOneAndDelete { .. }
        | CollectionOp::CreateIndex { .. } => Placement::Transaction,
        CollectionOp::Aggregate { pipeline, .. } => {
            pipeline_placement(&pipeline.iter().collect::<Vec<_>>())
        }
        CollectionOp::Admin { verb, .. } => match verb.as_str() {
            "createIndexes" => Placement::Transaction,
            "collStats" | "validate" => Placement::Outside,
            "dropIndexes" => Placement::Refused("dropping an index"),
            _ => Placement::Refused("changing a collection"),
        },
        CollectionOp::Drop => Placement::Refused("dropping a collection"),
        CollectionOp::DropIndex(_) => Placement::Refused("dropping an index"),
        CollectionOp::Rename { .. } => Placement::Refused("renaming a collection"),
        // `count`, `listIndexes`, `collStats`, `listCollections`, `explain`: refused in one.
        CollectionOp::EstimatedDocumentCount
        | CollectionOp::GetIndexes
        | CollectionOp::Stat(_)
        | CollectionOp::Exists
        | CollectionOp::Name { .. }
        | CollectionOp::Explain { .. } => Placement::Outside,
    }
}

/// The error of a statement refused in manual mode (see [`Placement::Refused`]).
#[must_use]
pub fn refused(what: &str) -> Error {
    Error::Unsupported(format!(
        "{what} in manual commit: MongoDB does not run it in a transaction. Switch to autocommit \
         to run it; nothing was sent, and the open transaction is unchanged"
    ))
}

/// How the server ended a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Because of the error of the statement being reported.
    ByThisError,
    /// Before the statement being reported, which found it gone.
    Before,
    /// It was open longer than `transactionLifetimeLimitSeconds`, `limit`.
    Expired { limit: u64 },
    /// The connection to the server was lost.
    Lost,
}

/// The message of `message` (the server's or the driver's) for a transaction ended `how`.
#[must_use]
pub fn ended_message(message: &str, how: Ended) -> String {
    match how {
        Ended::ByThisError => format!(
            "{message}. The server ended the transaction for this error, and its changes were \
             discarded"
        ),
        Ended::Before => format!(
            "The server had ended the transaction, and its changes were discarded ({message})"
        ),
        Ended::Expired { limit } => format!(
            "The server ended the transaction, and its changes were discarded: it was open longer \
             than the {limit} seconds MongoDB allows (transactionLifetimeLimitSeconds)"
        ),
        Ended::Lost => format!(
            "The connection to the server was lost during the transaction, which ended: its \
             changes were discarded ({message})"
        ),
    }
}

/// `e` with its message saying the server ended the transaction `how`.
#[must_use]
pub fn ended_error(e: Error, how: Ended) -> Error {
    match e {
        Error::Database(mut db) => {
            db.message = ended_message(&db.message, how);
            Error::Database(db)
        }
        other => {
            let message = match &other {
                Error::Other(m) | Error::Unsupported(m) | Error::Connect(m) => m.clone(),
                other => other.to_string(),
            };
            Error::Other(ended_message(&message, how))
        }
    }
}

/// What an answer of the server or the driver says about the transaction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Answer {
    /// The server's error code.
    pub code: Option<i32>,
    /// Labelled `TransientTransactionError`: the transaction is gone, and can be retried.
    pub transient: bool,
    /// The server could not be reached.
    pub network: bool,
    /// The driver refused it for the state of its transaction (already aborted, say).
    pub driver_state: bool,
}

/// `NoSuchTransaction`, `TransactionTooOld` and `TransactionExceededLifetimeLimitSeconds`.
const GONE_CODES: [i32; 3] = [251, 225, 290];

impl Answer {
    #[must_use]
    pub fn of(e: &mongodb::error::Error) -> Self {
        let code = match &*e.kind {
            ErrorKind::Command(c) => Some(c.code),
            ErrorKind::Write(mongodb::error::WriteFailure::WriteError(w)) => Some(w.code),
            ErrorKind::Write(mongodb::error::WriteFailure::WriteConcernError(w)) => Some(w.code),
            _ => None,
        };
        Self {
            code,
            transient: e.contains_label(TRANSIENT_TRANSACTION_ERROR),
            network: is_network(e),
            driver_state: matches!(&*e.kind, ErrorKind::Transaction { .. }),
        }
    }

    /// Whether it says the transaction no longer exists.
    #[must_use]
    pub fn ended(self) -> bool {
        self.transient || self.network || self.driver_state || self.gone()
    }

    /// Whether the transaction was gone before the operation that got this answer.
    #[must_use]
    pub fn gone(self) -> bool {
        self.code.is_some_and(|c| GONE_CODES.contains(&c))
    }
}

/// The first failure of the operations of a statement run in the transaction.
#[derive(Debug, Default)]
pub struct Failure {
    first: Option<Answer>,
    /// Some answer said the transaction had ended; with whether it was the first failure. One
    /// after another failure (the writes of an unordered `bulkWrite` go on) found it ended by
    /// that failure.
    ended: Option<(Answer, bool)>,
}

impl Failure {
    pub fn record(&mut self, e: &mongodb::error::Error) {
        self.record_answer(Answer::of(e));
    }

    fn record_answer(&mut self, answer: Answer) {
        let first = self.first.is_none();
        self.first.get_or_insert(answer);
        if answer.ended() {
            self.ended.get_or_insert((answer, first));
        }
    }
}

/// The transactions of a connection: whether the server runs them, the session of manual mode
/// and what is known of the transaction open in it.
#[derive(Debug)]
pub struct Transactions {
    /// The server is a replica set member or a `mongos`.
    pub supported: bool,
    state: TxTracker,
    /// Bumped when a transaction ends: a result read in one knows whether it is still open.
    generation: AtomicU64,
    /// When the open transaction started.
    started: Mutex<Option<Instant>>,
    /// The session of manual mode; `None` in autocommit mode. Locked for each statement, and
    /// for each batch of a result read in the transaction.
    pub session: tokio::sync::Mutex<Option<ClientSession>>,
}

/// The default of `transactionLifetimeLimitSeconds`.
const DEFAULT_LIFETIME: u64 = 60;

impl Transactions {
    #[must_use]
    pub fn new(supported: bool) -> Self {
        Self {
            supported,
            state: TxTracker::default(),
            generation: AtomicU64::new(0),
            started: Mutex::new(None),
            session: tokio::sync::Mutex::new(None),
        }
    }

    #[must_use]
    pub fn state(&self) -> TransactionState {
        self.state.state()
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Switches between autocommit and manual mode: manual mode holds a session of its own.
    pub async fn set_autocommit(&self, client: &mongodb::Client, on: bool) -> Result<()> {
        if !on && !self.supported {
            return Err(Error::Unsupported(
                "transactions need a replica set or a sharded cluster, and this server is \
                 standalone"
                    .to_owned(),
            ));
        }
        self.state.ensure_closed()?;
        if self.state().autocommit == on {
            return Ok(());
        }
        let mut session = self.session.lock().await;
        if on {
            *session = None;
        } else {
            let started = client.start_session().await.map_err(|e| map_error(&e))?;
            *session = Some(started);
        }
        self.state.set_autocommit(on);
        Ok(())
    }

    /// Opens a transaction in `session` if none is: the first statement after manual mode
    /// started, or after a commit or rollback. The generation the statement runs in.
    pub async fn begin(&self, session: &mut ClientSession) -> Result<u64> {
        if !self.state().open {
            // A transaction reads from the primary: one inherited from the connection's read
            // preference (`secondaryPreferred`, say) makes the driver refuse every read in it.
            async fn start(session: &mut ClientSession) -> mongodb::error::Result<()> {
                session
                    .start_transaction()
                    .selection_criteria(SelectionCriteria::ReadPreference(ReadPreference::Primary))
                    .await
            }
            if let Err(e) = start(session).await {
                // One the driver still holds as running, which this side had closed.
                if !Answer::of(&e).driver_state {
                    return Err(map_error(&e));
                }
                let _ = session.abort_transaction().await;
                start(session).await.map_err(|e| map_error(&e))?;
            }
            *self.started.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
            self.state.set_open(true);
        }
        Ok(self.generation())
    }

    /// Marks the transaction closed: the next statement opens another.
    fn close(&self) {
        self.state.set_open(false);
        *self.started.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Whether `generation` is the transaction open now.
    pub fn is_current(&self, generation: u64) -> bool {
        self.state().open && self.generation() == generation
    }

    pub async fn commit(&self) -> Result<()> {
        let mut guard = self.session.lock().await;
        let Some(session) = guard.as_mut().filter(|_| self.state().open) else {
            return Ok(());
        };
        let result = session.commit_transaction().await;
        let age = self.age();
        self.close();
        let Err(e) = result else { return Ok(()) };
        let answer = Answer::of(&e);
        let unknown = e.contains_label(UNKNOWN_TRANSACTION_COMMIT_RESULT);
        if answer.ended() && !unknown {
            let how = Self::how_ended(session, answer, age).await;
            return Err(ended_error(map_error(&e), how));
        }
        if unknown {
            return Err(map_error(&e).map_text(|m| {
                format!("{m}. Whether the server committed the transaction is unknown")
            }));
        }
        Err(map_error(&e))
    }

    pub async fn rollback(&self) -> Result<()> {
        let mut guard = self.session.lock().await;
        if let Some(session) = guard.as_mut().filter(|_| self.state().open) {
            // The driver ignores the server's errors: an aborted transaction is gone either way.
            let _ = session.abort_transaction().await;
            self.close();
        }
        Ok(())
    }

    /// After a statement of transaction `generation` was cancelled: what it did is unknown,
    /// and the server ends a transaction whose operation is killed, so it is rolled back.
    pub async fn lose(&self, session: &mut ClientSession, generation: u64) {
        if self.is_current(generation) {
            let _ = session.abort_transaction().await;
            self.close();
        }
    }

    /// After a statement of transaction `generation` failed: whether the server ended the
    /// transaction, and how. When no answer says so, a read that changes nothing asks, in
    /// database `db`: the server ends a transaction for some errors and not for others. An
    /// ended transaction is closed here too.
    pub async fn after_failure(
        &self,
        session: &mut ClientSession,
        generation: u64,
        failure: &Failure,
        db: &Database,
    ) -> Option<Ended> {
        failure.first?;
        if !self.is_current(generation) {
            return None;
        }
        let how = match failure.ended {
            Some((answer, _)) if answer.network => Ended::Lost,
            // Found gone at once: ended before the statement, or expired.
            Some((answer, true)) if answer.gone() => {
                Self::how_ended(session, answer, self.age()).await
            }
            Some((answer, true)) if answer.driver_state => Ended::Before,
            // Ended by its error, or by the one of an earlier failure of the statement.
            Some(_) => Ended::ByThisError,
            None => {
                let probe = db
                    .collection::<Document>(PROBE)
                    .find_one(doc! {})
                    .session(&mut *session)
                    .await;
                match probe {
                    Ok(_) => return None,
                    Err(e) if Answer::of(&e).network => Ended::Lost,
                    // Had it ended before, the statement would have been told so.
                    Err(_) => Ended::ByThisError,
                }
            }
        };
        let _ = session.abort_transaction().await;
        self.close();
        Some(how)
    }

    /// How long the open transaction has been open.
    fn age(&self) -> Option<Duration> {
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map(|t| t.elapsed())
    }

    /// For an `answer` that found the transaction gone: whether it outlived the server's
    /// limit, which is read only now, as it can change.
    async fn how_ended(session: &ClientSession, answer: Answer, age: Option<Duration>) -> Ended {
        if answer.network {
            return Ended::Lost;
        }
        let Some(age) = age else {
            return Ended::Before;
        };
        let limit = session
            .client()
            .database("admin")
            .run_command(doc! { "getParameter": 1, "transactionLifetimeLimitSeconds": 1 })
            .await
            .ok()
            .and_then(|reply| match reply.get("transactionLifetimeLimitSeconds") {
                Some(Bson::Int32(n)) => u64::try_from(*n).ok(),
                Some(Bson::Int64(n)) => u64::try_from(*n).ok(),
                _ => None,
            })
            .unwrap_or(DEFAULT_LIFETIME);
        if age.as_secs() >= limit {
            Ended::Expired { limit }
        } else {
            Ended::Before
        }
    }
}

/// The collection the read after a failure looks into: it needs no documents.
const PROBE: &str = "platypus.transaction.probe";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::parse;

    fn place(src: &str) -> Placement {
        placement(&parse(src).unwrap(), "shop")
    }

    #[test]
    fn documents_run_in_the_transaction() {
        for src in [
            "db.c.find({ a: 1 }).sort({ b: 1 })",
            "db.c.findOne()",
            "db.c.aggregate([{ $match: { a: 1 } }])",
            "db.c.countDocuments({})",
            "db.c.distinct('a')",
            "db.c.insertOne({ a: 1 })",
            "db.c.insertMany([{ a: 1 }])",
            "db.c.updateMany({}, { $set: { a: 1 } })",
            "db.c.replaceOne({ _id: 1 }, { a: 1 })",
            "db.c.deleteOne({ _id: 1 })",
            "db.c.bulkWrite([{ insertOne: { document: { a: 1 } } }])",
            "db.c.findOneAndUpdate({ _id: 1 }, { $set: { a: 1 } })",
            "db.c.findOneAndDelete({ _id: 1 })",
            "db.c.createIndex({ a: 1 })",
            "db.c.createIndexes([{ a: 1 }])",
            "db.createCollection('fresh')",
            "db.getSiblingDB('other').c.insertOne({ a: 1 })",
        ] {
            assert_eq!(place(src), Placement::Transaction, "{src}");
        }
    }

    #[test]
    fn metadata_and_administration_run_outside() {
        for src in [
            "show dbs",
            "show collections",
            "show users",
            "show profile",
            "use other",
            "db.getName()",
            "db.getCollectionNames()",
            "db.getCollectionInfos()",
            "db.stats()",
            "db.runCommand({ ping: 1 })",
            "db.adminCommand({ listDatabases: 1 })",
            "db.serverStatus()",
            "db.currentOp()",
            "db.getUsers()",
            "db.setProfilingLevel(1)",
            "rs.status()",
            "sh.status()",
            "db.c.stats()",
            "db.c.validate()",
            "db.c.dataSize()",
            "db.c.exists()",
            "db.c.getName()",
            "db.c.getIndexes()",
            "db.c.estimatedDocumentCount()",
            "db.c.latencyStats()",
            "db.c.aggregate([{ $indexStats: {} }])",
            "db.c.find().explain()",
            "db.system.views.find()",
            "db.getSiblingDB('admin').c.find()",
        ] {
            assert_eq!(place(src), Placement::Outside, "{src}");
        }
    }

    #[test]
    fn what_a_transaction_refuses_is_refused() {
        for src in [
            "db.c.drop()",
            "db.c.dropIndex('a_1')",
            "db.c.dropIndexes()",
            "db.c.renameCollection('d')",
            "db.c.hideIndex('a_1')",
            "db.c.convertToCapped(4096)",
            "db.c.aggregate([{ $match: {} }, { $out: 'd' }])",
            "db.c.aggregate([{ $merge: { into: 'd' } }])",
            "db.createView('v', 'c', [])",
            "db.createCollection('t', { timeseries: { timeField: 'at' } })",
            "db.getSiblingDB('admin').c.insertOne({ a: 1 })",
            "db.getSiblingDB('local').createCollection('c')",
            "db.system.js.insertOne({ a: 1 })",
        ] {
            assert!(
                matches!(place(src), Placement::Refused(_)),
                "{src}: {:?}",
                place(src)
            );
        }
        // The session's database counts when the statement names none.
        let insert = parse("db.c.insertOne({ a: 1 })").unwrap();
        assert!(matches!(placement(&insert, "admin"), Placement::Refused(_)));
        let message = refused("dropping a collection").to_string();
        assert!(
            message.contains("dropping a collection in manual commit"),
            "{message}"
        );
        assert!(
            message.contains("open transaction is unchanged"),
            "{message}"
        );
    }

    #[test]
    fn answers_tell_an_ended_transaction() {
        let gone = Answer {
            code: Some(251),
            transient: true,
            ..Answer::default()
        };
        assert!(gone.ended() && gone.gone());
        let conflict = Answer {
            code: Some(112),
            transient: true,
            ..Answer::default()
        };
        assert!(conflict.ended() && !conflict.gone());
        let duplicate = Answer {
            code: Some(11000),
            ..Answer::default()
        };
        assert!(!duplicate.ended(), "the server may keep it: asked after");
        let driver = Answer {
            driver_state: true,
            ..Answer::default()
        };
        assert!(driver.ended());
        let lost = Answer {
            network: true,
            ..Answer::default()
        };
        assert!(lost.ended());

        let mut failure = Failure::default();
        assert!(failure.first.is_none());
        failure.first = Some(duplicate);
        failure.ended.get_or_insert((gone, false));
        assert_eq!(failure.first, Some(duplicate));
    }

    #[test]
    fn server_commands_run_where_the_methods_that_send_them_do() {
        for src in [
            "db.runCommand({ insert: 'c', documents: [{ a: 1 }] })",
            "db.runCommand({ update: 'c', updates: [{ q: {}, u: { $set: { a: 1 } } }] })",
            "db.runCommand({ delete: 'c', deletes: [{ q: {}, limit: 0 }] })",
            "db.runCommand({ findAndModify: 'c', query: {}, remove: true })",
            "db.runCommand({ findandmodify: 'c', query: {}, remove: true })",
            "db.runCommand({ find: 'c', filter: {} })",
            "db.runCommand({ aggregate: 'c', pipeline: [], cursor: {} })",
            "db.runCommand({ distinct: 'c', key: 'a' })",
            "db.runCommand({ create: 'fresh' })",
            "db.runCommand({ createIndexes: 'c', indexes: [{ key: { a: 1 }, name: 'a_1' }] })",
            "db.getSiblingDB('other').runCommand({ insert: 'c', documents: [{ a: 1 }] })",
            "db.aggregate([{ $documents: [{ a: 1 }] }])",
        ] {
            assert_eq!(place(src), Placement::Transaction, "{src}");
        }
        for src in [
            "db.dropDatabase()",
            "db.runCommand({ drop: 'c' })",
            "db.runCommand({ collMod: 'c', validator: {} })",
            "db.runCommand({ dropIndexes: 'c', index: '*' })",
            "db.runCommand({ create: 'v', viewOn: 'c', pipeline: [] })",
            "db.runCommand({ aggregate: 'c', pipeline: [{ $out: 'd' }], cursor: {} })",
            "db.aggregate([{ $documents: [{ a: 1 }] }, { $merge: { into: 'd' } }])",
            "db.runCommand({ mapReduce: 'c', map: 'f', reduce: 'g', out: 'd' })",
            "db.adminCommand({ renameCollection: 'shop.c', to: 'shop.d' })",
            "db.adminCommand({ insert: 'c', documents: [{ a: 1 }] })",
            "db.runCommand({ insert: 'system.js', documents: [{ a: 1 }] })",
            "db.adminCommand({ bulkWrite: 1, ops: [], nsInfo: [] })",
        ] {
            assert!(
                matches!(place(src), Placement::Refused(_)),
                "{src}: {:?}",
                place(src)
            );
        }
        for src in [
            "db.runCommand({ count: 'c' })",
            "db.runCommand({ listIndexes: 'c' })",
            "db.runCommand({ explain: { delete: 'c', deletes: [{ q: {}, limit: 0 }] } })",
            "db.adminCommand({ find: 'system.version' })",
            "db.aggregate([{ $currentOp: {} }])",
            "db.runCommand({})",
        ] {
            assert_eq!(place(src), Placement::Outside, "{src}");
        }
    }

    #[test]
    fn a_failure_after_another_ended_the_transaction_with_it() {
        let duplicate = Answer {
            code: Some(11000),
            ..Answer::default()
        };
        let gone = Answer {
            code: Some(251),
            transient: true,
            ..Answer::default()
        };
        let mut failure = Failure::default();
        failure.record_answer(duplicate);
        failure.record_answer(gone);
        assert_eq!(failure.first, Some(duplicate));
        assert_eq!(failure.ended, Some((gone, false)), "ended by the duplicate");
        let mut failure = Failure::default();
        failure.record_answer(gone);
        failure.record_answer(duplicate);
        assert_eq!(failure.ended, Some((gone, true)), "found gone at once");
    }

    #[test]
    fn ended_transactions_say_their_changes_were_discarded() {
        let expired = ended_message(
            "Transaction has been aborted.",
            Ended::Expired { limit: 60 },
        );
        assert!(expired.contains("60 seconds"), "{expired}");
        assert!(expired.contains("discarded"), "{expired}");
        let own = ended_message("E11000 duplicate key", Ended::ByThisError);
        assert!(
            own.starts_with("E11000 duplicate key. The server ended"),
            "{own}"
        );
        let before = ended_message(
            "Transaction with { txnNumber: 2 } has been aborted.",
            Ended::Before,
        );
        assert!(
            before.contains("had ended") && before.contains("txnNumber"),
            "{before}"
        );
        let lost = ended_message("connection reset", Ended::Lost);
        assert!(
            lost.contains("lost") && lost.contains("discarded"),
            "{lost}"
        );

        let db = Error::Database(platypus_core::DbError {
            code: Some("NoSuchTransaction".to_owned()),
            message: "gone".to_owned(),
            ..platypus_core::DbError::default()
        });
        let Error::Database(db) = ended_error(db, Ended::Before) else {
            panic!("a database error stays one");
        };
        assert_eq!(db.code.as_deref(), Some("NoSuchTransaction"));
        assert!(db.message.contains("discarded (gone)"), "{}", db.message);
        let other = ended_error(Error::Other("reset".to_owned()), Ended::Lost);
        assert!(other.to_string().contains("(reset)"), "{other}");
    }
}
