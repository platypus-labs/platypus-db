//! The subset of the mongosh language the editor runs: `show dbs`, `use db`, the `db.*`
//! helpers and the collection methods with their cursor modifiers. A statement becomes a
//! [`Command`]; [`split`] cuts a script into statements.

use std::ops::Range;

use mongodb::bson::{Bson, Document, doc};

use crate::literal::{ParseError, ParseResult, Reader, integer, resolve_extended};

/// A parsed statement. `db` is `None` for the session's current database.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    ShowDatabases,
    ShowCollections,
    Use(String),
    /// `db.getCollectionNames()` (`names_only`) or `db.getCollectionInfos(filter)`.
    ListCollections {
        db: Option<String>,
        filter: Document,
        names_only: bool,
    },
    /// `db.getName()`.
    DatabaseName {
        db: Option<String>,
    },
    /// `db.runCommand(…)`, or `db.adminCommand(…)` with `db` set to `admin`.
    RunCommand {
        db: Option<String>,
        command: Document,
    },
    CreateCollection {
        db: Option<String>,
        name: String,
        options: Document,
    },
    Collection {
        db: Option<String>,
        collection: String,
        op: CollectionOp,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CollectionOp {
    Find(Find),
    Aggregate {
        pipeline: Vec<Document>,
        options: Document,
    },
    CountDocuments {
        filter: Document,
        skip: Option<u64>,
        limit: Option<u64>,
        options: Document,
    },
    EstimatedDocumentCount,
    Distinct {
        field: String,
        filter: Document,
        options: Document,
    },
    InsertOne(Document),
    InsertMany(Vec<Document>),
    /// Inserts with options: `ordered`, `writeConcern`, …
    InsertWith {
        documents: Vec<Document>,
        options: Document,
    },
    Update {
        filter: Document,
        /// An update document, or an aggregation pipeline.
        update: Bson,
        many: bool,
        upsert: bool,
        /// `arrayFilters`, `collation`, `hint`, `let`, …, as the server names them.
        options: Document,
    },
    Replace {
        filter: Document,
        replacement: Document,
        upsert: bool,
        options: Document,
    },
    Delete {
        filter: Document,
        many: bool,
        options: Document,
    },
    /// `bulkWrite`: writes of the kinds above, at least one. In order, stopping at the first
    /// that fails, unless not `ordered`. `options` go with each of them.
    BulkWrite {
        ops: Vec<CollectionOp>,
        ordered: bool,
        options: Document,
    },
    /// `findOneAndUpdate`, `findOneAndReplace` and `findOneAndDelete`: the document.
    FindOneAndUpdate {
        filter: Document,
        update: Bson,
        options: Document,
    },
    FindOneAndReplace {
        filter: Document,
        replacement: Document,
        options: Document,
    },
    FindOneAndDelete {
        filter: Document,
        options: Document,
    },
    /// A server command about this collection: `{ verb: <collection>, …rest }`.
    /// `changes_schema` also for what is too heavy to pass for a read: `validate` locks it.
    Admin {
        verb: String,
        rest: Document,
        changes_schema: bool,
    },
    /// What `dataSize()`, `storageSize()`, `totalIndexSize()`, `totalSize()` and `isCapped()`
    /// read from the collection's statistics.
    Stat(String),
    /// `exists()`: the collection's entry, if there is one.
    Exists,
    /// `getName()`, or with its database `getFullName()`.
    Name {
        full: bool,
    },
    /// The plan of an operation, which does not change anything.
    Explain {
        inner: Box<CollectionOp>,
        verbosity: String,
    },
    Drop,
    CreateIndex {
        keys: Document,
        options: Document,
    },
    DropIndex(Bson),
    GetIndexes,
    Rename {
        to: String,
        drop_target: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Find {
    pub filter: Document,
    pub projection: Option<Document>,
    pub sort: Option<Document>,
    pub skip: Option<u64>,
    pub limit: Option<i64>,
    /// `findOne`: at most one document.
    pub one: bool,
    /// `collation`, `hint`, `min`, `max`, …, as the server names them.
    pub options: Document,
}

/// One `.name` or `.name(args)` of a call chain.
#[derive(Debug)]
struct Segment {
    name: String,
    args: Option<Vec<Bson>>,
    offset: usize,
}

/// Parses one statement (without its trailing `;`).
pub fn parse(src: &str) -> ParseResult<Command> {
    let mut r = Reader::new(src);
    let start = {
        r.skip_space();
        r.pos
    };
    let Some(first) = r.ident() else {
        return r.error("expected `db`, `show` or `use`");
    };
    let command = match first {
        "show" => show(&mut r)?,
        "use" => {
            r.skip_space();
            let name = r.rest().trim().trim_end_matches(';').trim();
            let name = name.trim_matches(['"', '\'']);
            if name.is_empty() || name.contains(char::is_whitespace) {
                return r.error("expected a database name");
            }
            r.pos = src.len();
            Command::Use(name.to_owned())
        }
        "db" => db_chain(&mut r)?,
        "rs" | "sh" => cluster_helper(first == "rs", &mut r)?,
        _ => {
            r.pos = start;
            return r.error(format!(
                "unknown statement `{first}`: expected `db.`, `rs.`, `sh.`, `show` or `use`"
            ));
        }
    };
    r.eat(';');
    if !r.at_end() {
        return r.error("unexpected text after the statement");
    }
    Ok(command)
}

fn show(r: &mut Reader<'_>) -> ParseResult<Command> {
    let at = {
        r.skip_space();
        r.pos
    };
    match r.ident() {
        Some("dbs" | "databases") => Ok(Command::ShowDatabases),
        Some("collections" | "tables") => Ok(Command::ShowCollections),
        Some("users") => Ok(run_command(None, doc! { "usersInfo": 1 })),
        Some("roles") => Ok(run_command(None, doc! { "rolesInfo": 1 })),
        Some("logs") => Ok(admin_command(doc! { "getLog": "*" })),
        Some("log") => {
            r.skip_space();
            let name = r.ident().unwrap_or("global").to_owned();
            Ok(admin_command(doc! { "getLog": name }))
        }
        Some("profile") => Ok(Command::Collection {
            db: None,
            collection: "system.profile".to_owned(),
            op: CollectionOp::Find(Find {
                sort: Some(doc! { "ts": -1 }),
                limit: Some(5),
                ..Find::default()
            }),
        }),
        _ => {
            r.pos = at;
            r.error("expected `dbs`, `collections`, `users`, `roles`, `profile`, `logs` or `log`")
        }
    }
}

fn run_command(db: Option<String>, command: Document) -> Command {
    Command::RunCommand { db, command }
}

fn admin_command(command: Document) -> Command {
    run_command(Some("admin".to_owned()), command)
}

/// `rs.status()` and the like, the shell's names for server commands.
fn cluster_helper(replica_set: bool, r: &mut Reader<'_>) -> ParseResult<Command> {
    let mut chain = segments(r)?.into_iter();
    let Some(segment) = chain.next() else {
        return r.error("expected a method, e.g. `rs.status()`");
    };
    if let Some(extra) = chain.next() {
        return err(extra.offset, format!("unexpected `.{}`", extra.name));
    }
    let offset = segment.offset;
    let name = segment.name.clone();
    Args::new(segment, 0)?;
    let command = match (replica_set, name.as_str()) {
        (true, "status") => doc! { "replSetGetStatus": 1 },
        (true, "conf" | "config") => doc! { "replSetGetConfig": 1 },
        (true, "hello" | "isMaster") => doc! { "hello": 1 },
        (false, "status" | "listShards") => doc! { "listShards": 1 },
        (_, name) => {
            let prefix = if replica_set { "rs" } else { "sh" };
            return err(offset, format!("unknown method `{prefix}.{name}()`"));
        }
    };
    Ok(admin_command(command))
}

fn segments(r: &mut Reader<'_>) -> ParseResult<Vec<Segment>> {
    let mut out = Vec::new();
    loop {
        r.skip_space();
        let offset = r.pos;
        let name = if r.eat('.') {
            match r.ident() {
                Some(name) => name.to_owned(),
                None => return r.error("expected a name after `.`"),
            }
        } else if r.eat('[') {
            let name = r.string()?;
            r.expect(']')?;
            name
        } else {
            return Ok(out);
        };
        let args = if r.eat('(') {
            let mut args = Vec::new();
            while !r.eat(')') {
                args.push(resolve_extended(r.value()?));
                if !r.eat(',') {
                    r.expect(')')?;
                    break;
                }
            }
            Some(args)
        } else {
            None
        };
        out.push(Segment { name, args, offset });
    }
}

fn err<T>(offset: usize, message: impl Into<String>) -> ParseResult<T> {
    Err(ParseError {
        message: message.into(),
        offset,
    })
}

/// The arguments of a call segment, checked against how many it takes.
struct Args {
    items: std::vec::IntoIter<Bson>,
    method: String,
    offset: usize,
}

impl Args {
    fn new(segment: Segment, max: usize) -> ParseResult<Self> {
        let items = segment.args.unwrap_or_default();
        if items.len() > max {
            return err(
                segment.offset,
                format!("{}() takes at most {max} arguments", segment.name),
            );
        }
        Ok(Self {
            items: items.into_iter(),
            method: segment.name,
            offset: segment.offset,
        })
    }

    fn next(&mut self) -> Option<Bson> {
        self.items.next()
    }

    fn doc(&mut self, what: &str) -> ParseResult<Document> {
        match self.next() {
            Some(Bson::Document(d)) => Ok(d),
            _ => err(
                self.offset,
                format!("{}() expects {what} as a document", self.method),
            ),
        }
    }

    fn opt_doc(&mut self, what: &str) -> ParseResult<Document> {
        match self.next() {
            None | Some(Bson::Null | Bson::Undefined) => Ok(Document::new()),
            Some(Bson::Document(d)) => Ok(d),
            Some(_) => err(
                self.offset,
                format!("{}() expects {what} as a document", self.method),
            ),
        }
    }

    fn string(&mut self, what: &str) -> ParseResult<String> {
        match self.next() {
            Some(Bson::String(s)) => Ok(s),
            _ => err(
                self.offset,
                format!("{}() expects {what} as a string", self.method),
            ),
        }
    }

    fn docs(&mut self, what: &str) -> ParseResult<Vec<Document>> {
        let Some(Bson::Array(items)) = self.next() else {
            return err(
                self.offset,
                format!("{}() expects an array of {what}", self.method),
            );
        };
        items
            .into_iter()
            .map(|item| match item {
                Bson::Document(d) => Ok(d),
                _ => err(
                    self.offset,
                    format!("{}() expects an array of {what}", self.method),
                ),
            })
            .collect()
    }

    fn count(&mut self) -> ParseResult<i64> {
        match self.next() {
            Some(Bson::Int32(n)) => Ok(i64::from(n)),
            Some(Bson::Int64(n)) => Ok(n),
            #[allow(clippy::cast_possible_truncation)]
            Some(Bson::Double(n)) if n.fract() == 0.0 => Ok(n as i64),
            _ => err(self.offset, format!("{}() expects a number", self.method)),
        }
    }

    fn unsigned(&mut self) -> ParseResult<u64> {
        let offset = self.offset;
        let n = self.count()?;
        u64::try_from(n).or_else(|_| err(offset, "expected a positive number"))
    }
}

const FIND_OPTIONS: &[&str] = &[
    "allowDiskUse",
    "batchSize",
    "collation",
    "comment",
    "hint",
    "limit",
    "max",
    "maxTimeMS",
    "min",
    "noCursorTimeout",
    "projection",
    "readConcern",
    "returnKey",
    "showRecordId",
    "skip",
    "sort",
];
const AGGREGATE_OPTIONS: &[&str] = &[
    "allowDiskUse",
    "batchSize",
    "bypassDocumentValidation",
    "collation",
    "comment",
    "cursor",
    "hint",
    "let",
    "maxTimeMS",
    "readConcern",
    "writeConcern",
];
const COUNT_OPTIONS: &[&str] = &[
    "collation",
    "comment",
    "hint",
    "limit",
    "maxTimeMS",
    "readConcern",
    "skip",
];
const DISTINCT_OPTIONS: &[&str] = &["collation", "comment", "hint", "maxTimeMS", "readConcern"];
const INSERT_OPTIONS: &[&str] = &[
    "bypassDocumentValidation",
    "comment",
    "ordered",
    "writeConcern",
];
const UPDATE_OPTIONS: &[&str] = &[
    "arrayFilters",
    "bypassDocumentValidation",
    "collation",
    "comment",
    "hint",
    "let",
    "multi",
    "upsert",
    "writeConcern",
];
const REPLACE_OPTIONS: &[&str] = &[
    "bypassDocumentValidation",
    "collation",
    "comment",
    "hint",
    "let",
    "upsert",
    "writeConcern",
];
const DELETE_OPTIONS: &[&str] = &[
    "collation",
    "comment",
    "hint",
    "justOne",
    "let",
    "writeConcern",
];
const FIND_AND_MODIFY_OPTIONS: &[&str] = &[
    "arrayFilters",
    "bypassDocumentValidation",
    "collation",
    "comment",
    "hint",
    "let",
    "maxTimeMS",
    "projection",
    "returnDocument",
    "returnNewDocument",
    "sort",
    "upsert",
    "writeConcern",
];
const VERBOSITIES: [&str; 3] = ["queryPlanner", "executionStats", "allPlansExecution"];

/// `db…` after the `db` identifier.
fn db_chain(r: &mut Reader<'_>) -> ParseResult<Command> {
    let mut chain = segments(r)?.into_iter().peekable();
    let mut db = None;
    if chain
        .peek()
        .is_some_and(|s| s.name == "getSiblingDB" && s.args.is_some())
    {
        let mut args = Args::new(chain.next().unwrap_or_else(|| unreachable!()), 1)?;
        db = Some(args.string("the database name")?);
    }
    let Some(segment) = chain.next() else {
        return r.error("expected a method or collection after `db`");
    };
    if segment.args.is_some() && segment.name != "getCollection" {
        if let Some(extra) = chain.next() {
            return err(extra.offset, format!("unexpected `.{}`", extra.name));
        }
        return db_method(db, segment);
    }
    // A collection: `db.name`, `db.a.b` (named `a.b`) or `db.getCollection("name")`.
    let mut collection = if segment.args.is_some() {
        Args::new(segment, 1)?.string("the collection name")?
    } else {
        segment.name
    };
    let method = loop {
        match chain.next() {
            None => return r.error("expected a method call, e.g. `.find()`"),
            Some(s) if s.args.is_none() => {
                collection.push('.');
                collection.push_str(&s.name);
            }
            Some(s) => break s,
        }
    };
    let op = collection_op(method, chain.collect())?;
    Ok(Command::Collection { db, collection, op })
}

fn db_method(db: Option<String>, segment: Segment) -> ParseResult<Command> {
    let offset = segment.offset;
    Ok(match segment.name.as_str() {
        "getCollectionNames" => Command::ListCollections {
            db,
            filter: Document::new(),
            names_only: true,
        },
        "getCollectionInfos" => Command::ListCollections {
            db,
            filter: Args::new(segment, 1)?.opt_doc("the filter")?,
            names_only: false,
        },
        "runCommand" | "adminCommand" => {
            let admin = segment.name == "adminCommand";
            let mut args = Args::new(segment, 1)?;
            let command = match args.next() {
                Some(Bson::Document(d)) => d,
                // `db.runCommand("ping")` is `{ ping: 1 }`.
                Some(Bson::String(name)) => doc! { name: 1 },
                _ => return err(offset, "runCommand() expects a command document"),
            };
            Command::RunCommand {
                db: if admin { Some("admin".to_owned()) } else { db },
                command,
            }
        }
        "createCollection" | "createView" => {
            let view = segment.name == "createView";
            let mut args = Args::new(segment, if view { 3 } else { 2 })?;
            let name = args.string("the collection name")?;
            let options = if view {
                let source = args.string("the source collection")?;
                let pipeline = args.docs("pipeline stages")?;
                doc! { "viewOn": source, "pipeline": pipeline }
            } else {
                args.opt_doc("the options")?
            };
            Command::CreateCollection { db, name, options }
        }
        "dropDatabase" => Command::RunCommand {
            db,
            command: doc! { "dropDatabase": 1 },
        },
        "stats" => Command::RunCommand {
            db,
            command: doc! { "dbStats": 1 },
        },
        "version" => Command::RunCommand {
            db,
            command: doc! { "buildInfo": 1 },
        },
        "getName" => {
            Args::new(segment, 0)?;
            Command::DatabaseName { db }
        }
        "aggregate" => {
            let mut args = Args::new(segment, 2)?;
            let pipeline = args.docs("pipeline stages")?;
            let options = args.opt_doc("the options")?;
            only_keys(&options, AGGREGATE_OPTIONS, "db.aggregate()", offset)?;
            let mut command = doc! { "aggregate": 1, "pipeline": pipeline, "cursor": {} };
            command.extend(options.into_iter().filter(|(k, _)| k != "cursor"));
            run_command(db, command)
        }
        "serverStatus"
        | "hostInfo"
        | "hello"
        | "isMaster"
        | "listCommands"
        | "getProfilingStatus"
        | "getProfilingLevel"
        | "currentOp"
        | "killOp"
        | "setProfilingLevel"
        | "getUser"
        | "getUsers"
        | "createUser"
        | "updateUser"
        | "dropUser"
        | "grantRolesToUser"
        | "revokeRolesFromUser"
        | "getRole"
        | "getRoles"
        | "createRole"
        | "updateRole"
        | "dropRole" => db_helper(db, segment)?,
        "watch" => return err(offset, WATCH),
        name => return err(offset, format!("unknown method `db.{name}()`")),
    })
}

/// What refuses `watch()`, on a collection or on the database.
const WATCH: &str =
    "watch() is not available in the editor: a change stream would take over the connection";

/// The `db` methods that name a server command.
fn db_helper(db: Option<String>, segment: Segment) -> ParseResult<Command> {
    let name = segment.name.clone();
    let offset = segment.offset;
    let mut args = Args::new(segment, 3)?;
    let command = match name.as_str() {
        "serverStatus" => merged(doc! { "serverStatus": 1 }, args.opt_doc("the options")?),
        "hostInfo" => doc! { "hostInfo": 1 },
        "hello" | "isMaster" => doc! { "hello": 1 },
        "listCommands" => doc! { "listCommands": 1 },
        "getProfilingStatus" | "getProfilingLevel" => doc! { "profile": -1 },
        "setProfilingLevel" => {
            let level = args.count()?;
            merged(doc! { "profile": level }, args.opt_doc("the options")?)
        }
        "currentOp" => {
            return Ok(admin_command(merged(
                doc! { "currentOp": 1 },
                args.opt_doc("the filter")?,
            )));
        }
        "killOp" => {
            let Some(op) = args.next() else {
                return err(offset, "killOp() expects the operation's id");
            };
            return Ok(admin_command(doc! { "killOp": 1, "op": op }));
        }
        "getUsers" => merged(doc! { "usersInfo": 1 }, args.opt_doc("the options")?),
        "getRoles" => merged(doc! { "rolesInfo": 1 }, args.opt_doc("the options")?),
        "getUser" => {
            let user = args.string("the user")?;
            merged(doc! { "usersInfo": user }, args.opt_doc("the options")?)
        }
        "getRole" => {
            let role = args.string("the role")?;
            merged(doc! { "rolesInfo": role }, args.opt_doc("the options")?)
        }
        "createUser" | "createRole" => {
            let key = if name == "createUser" { "user" } else { "role" };
            let mut spec = args.doc("its definition")?;
            let Some(Bson::String(who)) = spec.remove(key) else {
                return err(offset, format!("{name}() needs `{key}`"));
            };
            merged(doc! { &name: who }, spec)
        }
        "updateUser" | "updateRole" => {
            let who = args.string("the name")?;
            merged(doc! { &name: who }, args.doc("the changes")?)
        }
        "dropUser" | "dropRole" => doc! { &name: args.string("the name")? },
        "grantRolesToUser" | "revokeRolesFromUser" => {
            let who = args.string("the user")?;
            let Some(roles @ Bson::Array(_)) = args.next() else {
                return err(offset, format!("{name}() expects an array of roles"));
            };
            doc! { &name: who, "roles": roles }
        }
        _ => unreachable!("not a helper: {name}"),
    };
    Ok(run_command(db, command))
}

/// `command` with `more` after it.
fn merged(mut command: Document, more: Document) -> Document {
    command.extend(more);
    command
}

fn collection_op(method: Segment, modifiers: Vec<Segment>) -> ParseResult<CollectionOp> {
    let name = method.name.clone();
    let offset = method.offset;
    let modifiers = modifiers.into_iter();
    let op = match name.as_str() {
        "find" | "findOne" => {
            let mut args = Args::new(method, 3)?;
            let mut find = Find {
                filter: args.opt_doc("the filter")?,
                projection: Some(args.opt_doc("the projection")?).filter(|p| !p.is_empty()),
                one: name == "findOne",
                ..Find::default()
            };
            take_find_options(&mut find, args.opt_doc("the options")?, offset)?;
            if find.one {
                find.limit = Some(1);
            } else {
                return cursor_modifiers(find, modifiers);
            }
            CollectionOp::Find(find)
        }
        "aggregate" => {
            let mut args = Args::new(method, 2)?;
            let pipeline = args.docs("pipeline stages")?;
            let options = args.opt_doc("the options")?;
            only_keys(&options, AGGREGATE_OPTIONS, "aggregate()", offset)?;
            CollectionOp::Aggregate { pipeline, options }
        }
        "countDocuments" | "count" => {
            let mut args = Args::new(method, 2)?;
            let filter = args.opt_doc("the filter")?;
            let mut options = args.opt_doc("the options")?;
            only_keys(&options, COUNT_OPTIONS, "countDocuments()", offset)?;
            let mut number = |key: &str| {
                options
                    .remove(key)
                    .as_ref()
                    .and_then(integer)
                    .and_then(|n| u64::try_from(n).ok())
            };
            let (skip, limit) = (number("skip"), number("limit"));
            CollectionOp::CountDocuments {
                skip,
                limit,
                filter,
                options,
            }
        }
        "estimatedDocumentCount" => {
            Args::new(method, 1)?;
            CollectionOp::EstimatedDocumentCount
        }
        "distinct" => {
            let mut args = Args::new(method, 3)?;
            let field = args.string("the field")?;
            let filter = args.opt_doc("the filter")?;
            let options = args.opt_doc("the options")?;
            only_keys(&options, DISTINCT_OPTIONS, "distinct()", offset)?;
            CollectionOp::Distinct {
                field,
                filter,
                options,
            }
        }
        "insertOne" | "insert" | "insertMany" | "updateOne" | "updateMany" | "update"
        | "replaceOne" | "deleteOne" | "deleteMany" | "remove" => write_op(&name, method)?,
        "bulkWrite" => bulk_write(method)?,
        "findOneAndUpdate" | "findOneAndReplace" | "findOneAndDelete" => {
            find_one_and(&name, method)?
        }
        "findAndModify" => find_and_modify(method)?,
        "explain" => return explain_first(method, modifiers),
        _ => admin_op(&name, method)?,
    };
    // `aggregate([…]).toArray()` and the like.
    let mut op = op;
    for m in modifiers {
        match m.name.as_str() {
            "toArray" | "pretty" => {}
            "explain" if explainable(&op) => {
                let verbosity = verbosity_of(m)?;
                op = CollectionOp::Explain {
                    inner: Box::new(op),
                    verbosity,
                };
            }
            _ => {
                return err(
                    m.offset,
                    format!("`.{}()` cannot follow `{name}()`", m.name),
                );
            }
        }
    }
    Ok(op)
}

/// The methods about the collection and its indexes, not its documents.
fn admin_op(name: &str, method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    Ok(match name {
        "createIndexes" => {
            let mut args = Args::new(method, 2)?;
            let keys = args.docs("index keys")?;
            let options = args.opt_doc("the options")?;
            let indexes: Vec<Document> = keys
                .into_iter()
                .map(|keys| {
                    let name = crate::meta::index_name(&keys);
                    let mut spec = doc! { "key": keys, "name": name };
                    spec.extend(options.clone());
                    spec
                })
                .collect();
            admin("createIndexes", doc! { "indexes": indexes }, true)
        }
        "dropIndexes" => {
            let index = match Args::new(method, 1)?.next() {
                None => Bson::String("*".to_owned()),
                Some(index @ (Bson::String(_) | Bson::Document(_) | Bson::Array(_))) => index,
                Some(_) => return err(offset, "dropIndexes() expects index names or keys"),
            };
            admin("dropIndexes", doc! { "index": index }, true)
        }
        "hideIndex" | "unhideIndex" => {
            let index = match Args::new(method, 1)?.next() {
                Some(Bson::String(index)) => doc! { "name": index, "hidden": name == "hideIndex" },
                Some(Bson::Document(keys)) => {
                    doc! { "keyPattern": keys, "hidden": name == "hideIndex" }
                }
                _ => return err(offset, format!("{name}() expects an index name or keys")),
            };
            admin("collMod", doc! { "index": index }, true)
        }
        "convertToCapped" => {
            let size = Args::new(method, 1)?.count()?;
            admin("convertToCapped", doc! { "size": size }, true)
        }
        "watch" => return err(offset, WATCH),
        "mapReduce" => {
            return err(
                offset,
                "mapReduce() takes JavaScript functions, which the editor does not run: use aggregate()",
            );
        }
        "initializeOrderedBulkOp" | "initializeUnorderedBulkOp" => {
            return err(
                offset,
                format!(
                    "{name}() builds its writes with JavaScript, which the editor does not run: use bulkWrite()"
                ),
            );
        }
        "drop" => {
            Args::new(method, 0)?;
            CollectionOp::Drop
        }
        "createIndex" | "ensureIndex" => {
            let mut args = Args::new(method, 2)?;
            let keys = args.doc("the index keys")?;
            CollectionOp::CreateIndex {
                keys,
                options: args.opt_doc("the options")?,
            }
        }
        "dropIndex" => {
            let mut args = Args::new(method, 1)?;
            match args.next() {
                Some(index @ (Bson::String(_) | Bson::Document(_))) => {
                    CollectionOp::DropIndex(index)
                }
                _ => return err(offset, "dropIndex() expects an index name or key document"),
            }
        }
        "renameCollection" => {
            let mut args = Args::new(method, 2)?;
            let to = args.string("the new name")?;
            CollectionOp::Rename {
                to,
                drop_target: matches!(args.next(), Some(Bson::Boolean(true))),
            }
        }
        "getIndexes" | "getIndices" | "getIndexKeys" | "getIndexSpecs" => {
            Args::new(method, 0)?;
            CollectionOp::GetIndexes
        }
        _ => info_op(name, method)?,
    })
}

/// What reads about the collection: its statistics and its names.
fn info_op(name: &str, method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    Ok(match name {
        "stats" => {
            let options = Args::new(method, 1)?.opt_doc("the options")?;
            admin("collStats", options, false)
        }
        "validate" => {
            let options = Args::new(method, 1)?.opt_doc("the options")?;
            admin("validate", options, true)
        }
        "latencyStats" => {
            let options = Args::new(method, 1)?.opt_doc("the options")?;
            CollectionOp::Aggregate {
                pipeline: vec![doc! { "$collStats": { "latencyStats": options } }],
                options: Document::new(),
            }
        }
        "dataSize" | "storageSize" | "totalIndexSize" | "totalSize" | "isCapped" => {
            Args::new(method, 0)?;
            CollectionOp::Stat(name.to_owned())
        }
        "exists" => {
            Args::new(method, 0)?;
            CollectionOp::Exists
        }
        "getName" | "getFullName" => {
            Args::new(method, 0)?;
            CollectionOp::Name {
                full: name == "getFullName",
            }
        }
        other => return err(offset, format!("unknown collection method `{other}()`")),
    })
}

fn admin(verb: &str, rest: Document, changes_schema: bool) -> CollectionOp {
    CollectionOp::Admin {
        verb: verb.to_owned(),
        rest,
        changes_schema,
    }
}

/// The verbosity an `explain()` asks for: the default, the shell's `true` for all plans, a name.
fn verbosity_of(segment: Segment) -> ParseResult<String> {
    let offset = segment.offset;
    match Args::new(segment, 1)?.next() {
        None | Some(Bson::Boolean(false)) => Ok(VERBOSITIES[0].to_owned()),
        Some(Bson::Boolean(true)) => Ok(VERBOSITIES[2].to_owned()),
        Some(Bson::String(name)) if VERBOSITIES.contains(&name.as_str()) => Ok(name),
        Some(_) => err(
            offset,
            "explain() expects queryPlanner, executionStats or allPlansExecution",
        ),
    }
}

/// `db.c.explain("executionStats").find(…).sort(…)`: the method after it is what is explained.
fn explain_first(
    method: Segment,
    modifiers: std::vec::IntoIter<Segment>,
) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    let verbosity = verbosity_of(method)?;
    let mut rest = modifiers;
    let Some(next) = rest.next() else {
        return err(
            offset,
            "explain() needs what to explain, e.g. `.explain().find()`",
        );
    };
    let inner = collection_op(next, rest.collect())?;
    if !explainable(&inner) {
        return err(
            offset,
            "explain() takes find, aggregate, count, distinct and writes of documents",
        );
    }
    Ok(CollectionOp::Explain {
        inner: Box::new(inner),
        verbosity,
    })
}

/// Whether the server can give the plan of `op`.
#[must_use]
pub fn explainable(op: &CollectionOp) -> bool {
    matches!(
        op,
        CollectionOp::Find(_)
            | CollectionOp::Aggregate { .. }
            | CollectionOp::CountDocuments { .. }
            | CollectionOp::Distinct { .. }
            | CollectionOp::Update { .. }
            | CollectionOp::Replace { .. }
            | CollectionOp::Delete { .. }
            | CollectionOp::FindOneAndUpdate { .. }
            | CollectionOp::FindOneAndReplace { .. }
            | CollectionOp::FindOneAndDelete { .. }
    )
}

/// `find`'s third argument, or a cursor method: what the statement holds apart from the rest.
fn take_find_options(find: &mut Find, mut options: Document, offset: usize) -> ParseResult<()> {
    only_keys(&options, FIND_OPTIONS, "find()", offset)?;
    if let Some(Bson::Document(sort)) = options.remove("sort") {
        find.sort = Some(sort);
    }
    if let Some(Bson::Document(projection)) = options.remove("projection") {
        find.projection = Some(projection);
    }
    if let Some(skip) = options.remove("skip").as_ref().and_then(integer) {
        find.skip = u64::try_from(skip).ok();
    }
    if let Some(limit) = options.remove("limit").as_ref().and_then(integer) {
        find.limit = Some(limit).filter(|&n| n != 0);
    }
    find.options.extend(options);
    Ok(())
}

/// `findOneAndUpdate`, `findOneAndReplace` and `findOneAndDelete`.
fn find_one_and(name: &str, method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    let delete = name == "findOneAndDelete";
    let mut args = Args::new(method, if delete { 2 } else { 3 })?;
    let filter = args.doc("the filter")?;
    Ok(match name {
        "findOneAndDelete" => CollectionOp::FindOneAndDelete {
            filter,
            options: find_and_options(args.opt_doc("the options")?, name, offset)?,
        },
        "findOneAndReplace" => {
            let replacement = args.doc("the replacement")?;
            CollectionOp::FindOneAndReplace {
                filter,
                replacement,
                options: find_and_options(args.opt_doc("the options")?, name, offset)?,
            }
        }
        _ => {
            let Some(update @ (Bson::Document(_) | Bson::Array(_))) = args.next() else {
                return err(offset, format!("{name}() expects an update document"));
            };
            CollectionOp::FindOneAndUpdate {
                filter,
                update,
                options: find_and_options(args.opt_doc("the options")?, name, offset)?,
            }
        }
    })
}

/// The options of a `findOneAnd…`, with the shell's older `returnNewDocument` as the driver's.
fn find_and_options(mut options: Document, name: &str, offset: usize) -> ParseResult<Document> {
    only_keys(&options, FIND_AND_MODIFY_OPTIONS, name, offset)?;
    if flag(&options, "returnNewDocument", false, offset)? {
        options.insert("returnDocument", "after");
    }
    options.remove("returnNewDocument");
    Ok(options)
}

/// `findAndModify({ query, sort, update | remove, new, fields, upsert, … })`.
fn find_and_modify(method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    let mut spec = Args::new(method, 1)?.doc("its specification")?;
    let filter = match spec.remove("query") {
        Some(Bson::Document(query)) => query,
        None => Document::new(),
        Some(_) => return err(offset, "`query` must be a document"),
    };
    let remove = flag(&spec, "remove", false, offset)?;
    spec.remove("remove");
    if flag(&spec, "new", false, offset)? {
        spec.insert("returnDocument", "after");
    }
    spec.remove("new");
    if let Some(fields) = spec.remove("fields") {
        spec.insert("projection", fields);
    }
    let update = spec.remove("update");
    let options = find_and_options(spec, "findAndModify()", offset)?;
    match (remove, update) {
        (true, None) => Ok(CollectionOp::FindOneAndDelete { filter, options }),
        (false, Some(update @ (Bson::Document(_) | Bson::Array(_)))) => {
            Ok(CollectionOp::FindOneAndUpdate {
                filter,
                update,
                options,
            })
        }
        _ => err(
            offset,
            "findAndModify() needs `update` or `remove: true`, not both",
        ),
    }
}

/// The collection methods that change documents.
fn write_op(name: &str, method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    Ok(match name {
        "insertOne" | "insert" | "insertMany" => {
            let mut args = Args::new(method, 2)?;
            let documents = args.next();
            let options = args.opt_doc("the options")?;
            only_keys(&options, INSERT_OPTIONS, name, offset)?;
            let op = match documents {
                Some(Bson::Document(d)) if name != "insertMany" => CollectionOp::InsertOne(d),
                Some(Bson::Array(items)) if name != "insertOne" => CollectionOp::InsertMany(
                    items
                        .into_iter()
                        .map(|item| match item {
                            Bson::Document(d) => Ok(d),
                            _ => err(offset, "insertMany() expects an array of documents"),
                        })
                        .collect::<ParseResult<_>>()?,
                ),
                _ => return err(offset, format!("{name}() expects documents")),
            };
            match (op, options.is_empty()) {
                (op, true) => op,
                (CollectionOp::InsertOne(document), false) => CollectionOp::InsertWith {
                    documents: vec![document],
                    options,
                },
                (CollectionOp::InsertMany(documents), false) => {
                    CollectionOp::InsertWith { documents, options }
                }
                _ => unreachable!("only inserts above"),
            }
        }
        "updateOne" | "updateMany" | "update" => {
            let mut args = Args::new(method, 3)?;
            let filter = args.doc("the filter")?;
            let Some(update @ (Bson::Document(_) | Bson::Array(_))) = args.next() else {
                return err(offset, format!("{name}() expects an update document"));
            };
            let mut options = args.opt_doc("the options")?;
            only_keys(&options, UPDATE_OPTIONS, name, offset)?;
            // Only the old `update()` takes it: on `updateOne` it would change every match.
            if name != "update" && options.contains_key("multi") {
                return err(offset, format!("`multi` is not supported in {name}"));
            }
            let many = name == "updateMany" || flag(&options, "multi", false, offset)?;
            let upsert = flag(&options, "upsert", false, offset)?;
            options.remove("multi");
            options.remove("upsert");
            CollectionOp::Update {
                filter,
                update,
                many,
                upsert,
                options,
            }
        }
        "replaceOne" => {
            let mut args = Args::new(method, 3)?;
            let filter = args.doc("the filter")?;
            let replacement = args.doc("the replacement")?;
            let mut options = args.opt_doc("the options")?;
            only_keys(&options, REPLACE_OPTIONS, name, offset)?;
            let upsert = flag(&options, "upsert", false, offset)?;
            options.remove("upsert");
            CollectionOp::Replace {
                filter,
                replacement,
                upsert,
                options,
            }
        }
        "deleteOne" | "deleteMany" | "remove" => {
            let mut args = Args::new(method, 2)?;
            let filter = args.doc("the filter")?;
            let mut options = args.opt_doc("the options")?;
            only_keys(&options, DELETE_OPTIONS, name, offset)?;
            let many = name == "deleteMany"
                || (name == "remove" && !flag(&options, "justOne", false, offset)?);
            options.remove("justOne");
            CollectionOp::Delete {
                filter,
                many,
                options,
            }
        }
        _ => unreachable!("not a write method: {name}"),
    })
}

/// `bulkWrite([{ insertOne: { document } }, { updateMany: { filter, update } }, …], { ordered })`.
/// An option this does not carry out is refused, not dropped: an update that lost its
/// `arrayFilters` would change other values than the ones asked for.
fn bulk_write(method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    let mut args = Args::new(method, 2)?;
    let Some(Bson::Array(items)) = args.next() else {
        return err(offset, "bulkWrite() expects an array of operations");
    };
    let mut options = args.opt_doc("the options")?;
    only_keys(
        &options,
        &["comment", "ordered", "writeConcern"],
        "bulkWrite()",
        offset,
    )?;
    let ordered = flag(&options, "ordered", true, offset)?;
    options.remove("ordered");
    if items.is_empty() {
        return err(offset, "bulkWrite() expects at least one operation");
    }
    let ops = items
        .into_iter()
        .map(|item| bulk_operation(item, offset))
        .collect::<ParseResult<_>>()?;
    Ok(CollectionOp::BulkWrite {
        ops,
        ordered,
        options,
    })
}

/// One `{ updateOne: { … } }` of a `bulkWrite`.
fn bulk_operation(item: Bson, offset: usize) -> ParseResult<CollectionOp> {
    const SHAPE: &str = "each bulkWrite() operation is one of insertOne, updateOne, updateMany, replaceOne, deleteOne or deleteMany, as { deleteOne: { filter: … } }";
    let Bson::Document(item) = item else {
        return err(offset, SHAPE);
    };
    let mut entries = item.into_iter();
    let (Some((name, Bson::Document(body))), None) = (entries.next(), entries.next()) else {
        return err(offset, SHAPE);
    };
    let known: &[&str] = match name.as_str() {
        "insertOne" => &["document"],
        "updateOne" | "updateMany" => &[
            "filter",
            "update",
            "upsert",
            "arrayFilters",
            "collation",
            "hint",
        ],
        "replaceOne" => &["filter", "replacement", "upsert", "collation", "hint"],
        "deleteOne" | "deleteMany" => &["filter", "collation", "hint"],
        _ => return err(offset, SHAPE),
    };
    only_keys(&body, known, &name, offset)?;
    let upsert = flag(&body, "upsert", false, offset)?;
    let part = |key: &str| match body.get(key) {
        Some(Bson::Document(d)) => Ok(d.clone()),
        _ => err(offset, format!("{name} in bulkWrite() needs `{key}`")),
    };
    // What is left of the body, once the parts above are out, goes to the server as it is.
    let options = || -> Document {
        body.iter()
            .filter(|(k, _)| {
                !matches!(
                    k.as_str(),
                    "filter" | "update" | "replacement" | "upsert" | "document"
                )
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    Ok(match name.as_str() {
        "insertOne" => CollectionOp::InsertOne(part("document")?),
        "updateOne" | "updateMany" => CollectionOp::Update {
            filter: part("filter")?,
            update: match body.get("update") {
                Some(update @ (Bson::Document(_) | Bson::Array(_))) => update.clone(),
                _ => return err(offset, format!("{name} in bulkWrite() needs `update`")),
            },
            many: name == "updateMany",
            upsert,
            options: options(),
        },
        "replaceOne" => CollectionOp::Replace {
            filter: part("filter")?,
            replacement: part("replacement")?,
            upsert,
            options: options(),
        },
        _ => CollectionOp::Delete {
            filter: part("filter")?,
            many: name == "deleteMany",
            options: options(),
        },
    })
}

/// Refuses the keys of `body` other than `known`, by name.
fn only_keys(body: &Document, known: &[&str], what: &str, offset: usize) -> ParseResult<()> {
    match body.keys().find(|k| !known.contains(&k.as_str())) {
        Some(key) => err(offset, format!("`{key}` is not supported in {what}")),
        None => Ok(()),
    }
}

/// The boolean option `key` of `body`, `default` when it is not there.
fn flag(body: &Document, key: &str, default: bool, offset: usize) -> ParseResult<bool> {
    match body.get(key) {
        None => Ok(default),
        Some(Bson::Boolean(value)) => Ok(*value),
        Some(_) => err(offset, format!("`{key}` must be true or false")),
    }
}

/// The writes of a `bulkWrite` as they are sent, each with how many operations it stands for:
/// neighbouring inserts of an ordered one go together, a single round trip. An unordered one
/// runs each alone, so that one refused document does not take its neighbours with it.
#[must_use]
pub fn write_batches(ops: Vec<CollectionOp>, ordered: bool) -> Vec<(CollectionOp, usize)> {
    let mut out: Vec<(CollectionOp, usize)> = Vec::with_capacity(ops.len());
    for op in ops {
        match (op, out.last_mut()) {
            (CollectionOp::InsertOne(next), Some((CollectionOp::InsertMany(docs), size)))
                if ordered =>
            {
                docs.push(next);
                *size += 1;
            }
            (CollectionOp::InsertOne(next), Some(last))
                if ordered && matches!(last.0, CollectionOp::InsertOne(_)) =>
            {
                let CollectionOp::InsertOne(first) =
                    std::mem::replace(&mut last.0, CollectionOp::Drop)
                else {
                    unreachable!("checked above");
                };
                *last = (CollectionOp::InsertMany(vec![first, next]), 2);
            }
            (op, _) => out.push((op, 1)),
        }
    }
    out
}

fn cursor_modifiers(
    mut find: Find,
    modifiers: impl Iterator<Item = Segment>,
) -> ParseResult<CollectionOp> {
    for m in modifiers {
        let offset = m.offset;
        if m.args.is_none() {
            return err(offset, format!("`.{}` must be called", m.name));
        }
        match m.name.as_str() {
            "sort" => find.sort = Some(Args::new(m, 1)?.doc("the sort")?),
            "projection" => find.projection = Some(Args::new(m, 1)?.doc("the projection")?),
            "limit" => find.limit = Some(Args::new(m, 1)?.count()?).filter(|&n| n != 0),
            "skip" => find.skip = Some(Args::new(m, 1)?.unsigned()?),
            "count" | "itcount" | "size" => {
                let options = std::mem::take(&mut find.options)
                    .into_iter()
                    .filter(|(k, _)| COUNT_OPTIONS.contains(&k.as_str()))
                    .collect();
                return Ok(CollectionOp::CountDocuments {
                    filter: find.filter,
                    skip: find.skip,
                    limit: find.limit.and_then(|n| u64::try_from(n.abs()).ok()),
                    options,
                });
            }
            "explain" => {
                let verbosity = verbosity_of(m)?;
                return Ok(CollectionOp::Explain {
                    inner: Box::new(CollectionOp::Find(find)),
                    verbosity,
                });
            }
            // A value the server takes as it is.
            name @ ("collation" | "hint" | "min" | "max" | "maxTimeMS") => {
                let name = name.to_owned();
                let Some(value) = Args::new(m, 1)?.next() else {
                    return err(offset, format!("`.{name}()` expects a value"));
                };
                find.options.insert(name, value);
            }
            "readConcern" => {
                let Some(Bson::String(level)) = Args::new(m, 1)?.next() else {
                    return err(offset, "`.readConcern()` expects a level");
                };
                find.options.insert("readConcern", doc! { "level": level });
            }
            // On unless told otherwise.
            name @ ("allowDiskUse" | "noCursorTimeout" | "returnKey" | "showRecordId") => {
                let name = name.to_owned();
                let on = !matches!(Args::new(m, 1)?.next(), Some(Bson::Boolean(false)));
                find.options.insert(name, on);
            }
            // How the results travel, which the app decides; `comment` carries the app's own,
            // that finds the operation to cancel it.
            "toArray" | "pretty" | "batchSize" | "comment" => {}
            name @ ("forEach" | "map" | "next" | "hasNext" | "tryNext" | "tailable"
            | "addOption") => {
                return err(
                    offset,
                    format!(
                        "`.{name}()` is not available in the editor, which runs statements, not JavaScript"
                    ),
                );
            }
            other => return err(offset, format!("unknown cursor method `.{other}()`")),
        }
    }
    Ok(CollectionOp::Find(find))
}

/// What a statement does, for the checks the editor makes before running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// Reads documents or metadata.
    Query,
    /// Writes documents.
    Dml,
    /// Creates, drops or renames collections and indexes.
    Ddl,
    /// `runCommand`, which can do anything.
    Other,
}

#[must_use]
pub fn kind(command: &Command) -> StatementKind {
    if is_read_only(command) {
        return StatementKind::Query;
    }
    match command {
        Command::CreateCollection { .. } => StatementKind::Ddl,
        Command::Collection { op, .. } => match op {
            CollectionOp::Drop
            | CollectionOp::CreateIndex { .. }
            | CollectionOp::DropIndex(_)
            | CollectionOp::Rename { .. }
            | CollectionOp::Admin { .. } => StatementKind::Ddl,
            _ => StatementKind::Dml,
        },
        _ => StatementKind::Other,
    }
}

/// The `(database, collection)` that a statement changes in every document: an
/// `updateMany` or `deleteMany` with an empty filter, alone or inside a `bulkWrite`.
#[must_use]
pub fn unfiltered(command: &Command) -> Option<(Option<&str>, &str)> {
    let Command::Collection { db, collection, op } = command else {
        return None;
    };
    changes_every_document(op).then_some((db.as_deref(), collection.as_str()))
}

fn changes_every_document(op: &CollectionOp) -> bool {
    match op {
        CollectionOp::Update { filter, many, .. } | CollectionOp::Delete { filter, many, .. } => {
            *many && filter.is_empty()
        }
        CollectionOp::BulkWrite { ops, .. } => ops.iter().any(changes_every_document),
        _ => false,
    }
}

/// Whether a statement only reads.
#[must_use]
pub fn is_read_only(command: &Command) -> bool {
    match command {
        Command::ShowDatabases
        | Command::ShowCollections
        | Command::Use(_)
        | Command::DatabaseName { .. }
        | Command::ListCollections { .. } => true,
        Command::RunCommand { .. } | Command::CreateCollection { .. } => false,
        Command::Collection { op, .. } => op_reads(op),
    }
}

fn op_reads(op: &CollectionOp) -> bool {
    match op {
        CollectionOp::Find(_)
        | CollectionOp::CountDocuments { .. }
        | CollectionOp::EstimatedDocumentCount
        | CollectionOp::Distinct { .. }
        | CollectionOp::GetIndexes
        | CollectionOp::Stat(_)
        | CollectionOp::Exists
        | CollectionOp::Name { .. } => true,
        CollectionOp::Admin { changes_schema, .. } => !changes_schema,
        CollectionOp::Aggregate { pipeline, .. } => !pipeline
            .iter()
            .any(|s| s.contains_key("$out") || s.contains_key("$merge")),
        // A plan changes nothing, whatever it is the plan of; an aggregate that writes keeps
        // being treated as a write, for the verbosities that run its stages.
        CollectionOp::Explain { inner, .. } => {
            !matches!(&**inner, CollectionOp::Aggregate { .. }) || op_reads(inner)
        }
        _ => false,
    }
}

/// The statements of a script as byte ranges, without surrounding space and `;`.
///
/// A statement ends at a `;`, or at a line break outside brackets when the next line does
/// not continue it with `.` (a chained call).
#[must_use]
pub fn split(src: &str) -> Vec<Range<usize>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    // The last significant character, to tell a regex `/` from a division.
    let mut last = b'(';
    let push = |out: &mut Vec<Range<usize>>, from: usize, to: usize| {
        let text = &src[from..to];
        let lead = text.len() - text.trim_start().len();
        let trimmed = text.trim();
        if !trimmed.is_empty() && !is_comment_only(trimmed) {
            out.push(from + lead..from + lead + trimmed.len());
        }
    };
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' | b'\'' | b'`' => {
                i = skip_string(bytes, i);
                last = c;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = src[i..].find('\n').map_or(bytes.len(), |n| i + n);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..].find("*/").map_or(bytes.len(), |n| i + n + 4);
                continue;
            }
            b'/' if b"(,:[{=!&|?;".contains(&last) => {
                i = skip_regex(bytes, i);
                last = b'/';
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            // A `;`, or a line break after a statement that the next line does not continue.
            b';' | b'\n'
                if depth == 0
                    && (c == b';'
                        || (!continues(&src[i + 1..]) && !src[start..i].trim().is_empty())) =>
            {
                push(&mut out, start, i);
                start = i + 1;
                last = b';';
                i += 1;
                continue;
            }
            _ => {}
        }
        if !c.is_ascii_whitespace() {
            last = c;
        }
        i += 1;
    }
    push(&mut out, start, src.len());
    out
}

/// The statement under `cursor` (a byte offset), or the one ending on its line before it.
#[must_use]
pub fn statement_at(src: &str, cursor: usize) -> Option<Range<usize>> {
    let statements = split(src);
    statements
        .iter()
        .find(|r| r.start <= cursor && cursor <= r.end)
        .or_else(|| {
            statements
                .iter()
                .rev()
                .find(|r| r.end <= cursor && !src[r.end..cursor].contains('\n'))
        })
        .cloned()
}

fn is_comment_only(text: &str) -> bool {
    let mut r = Reader::new(text);
    r.at_end()
}

/// Whether the text after a line break continues the statement: a `.method()` chain.
fn continues(rest: &str) -> bool {
    let mut r = Reader::new(rest);
    r.skip_space();
    r.peek() == Some('.')
}

fn skip_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            c if c == quote => return i + 1,
            b'\n' if quote != b'`' => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_regex(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class => return i + 1,
            b'\n' => return i,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use mongodb::bson::doc;

    use super::*;

    fn op(src: &str) -> CollectionOp {
        match parse(src).unwrap_or_else(|e| panic!("{src}: {e:?}")) {
            Command::Collection { op, .. } => op,
            other => panic!("{src}: {other:?}"),
        }
    }

    fn find(src: &str) -> Find {
        match op(src) {
            CollectionOp::Find(f) => f,
            other => panic!("{src}: {other:?}"),
        }
    }

    fn texts(src: &str) -> Vec<&str> {
        split(src).into_iter().map(|r| &src[r]).collect()
    }

    #[test]
    fn parses_shell_commands() {
        assert_eq!(parse("show dbs").unwrap(), Command::ShowDatabases);
        assert_eq!(
            parse("show collections;").unwrap(),
            Command::ShowCollections
        );
        assert_eq!(parse("use shop").unwrap(), Command::Use("shop".into()));
        assert!(parse("show nope").is_err());
        assert!(
            parse("select 1")
                .unwrap_err()
                .message
                .contains("unknown statement")
        );
    }

    #[test]
    fn parses_database_methods() {
        assert_eq!(
            parse("db.getCollectionNames()").unwrap(),
            Command::ListCollections {
                db: None,
                filter: doc! {},
                names_only: true
            }
        );
        assert_eq!(
            parse("db.runCommand({ ping: 1 })").unwrap(),
            Command::RunCommand {
                db: None,
                command: doc! { "ping": 1 }
            }
        );
        assert_eq!(
            parse("db.adminCommand('listDatabases')").unwrap(),
            Command::RunCommand {
                db: Some("admin".into()),
                command: doc! { "listDatabases": 1 }
            }
        );
        assert_eq!(
            parse(
                "db.getSiblingDB('other').createCollection('logs', { capped: true, size: 4096 })"
            )
            .unwrap(),
            Command::CreateCollection {
                db: Some("other".into()),
                name: "logs".into(),
                options: doc! { "capped": true, "size": 4096 }
            }
        );
        assert!(parse("db.nope()").unwrap_err().message.contains("db.nope"));
    }

    #[test]
    fn parses_find_with_modifiers() {
        let f = find(
            "db.users.find({ age: { $gt: 30 } }, { name: 1 }).sort({ age: -1 }).skip(5).limit(10)",
        );
        assert_eq!(f.filter, doc! { "age": { "$gt": 30 } });
        assert_eq!(f.projection, Some(doc! { "name": 1 }));
        assert_eq!(f.sort, Some(doc! { "age": -1 }));
        assert_eq!((f.skip, f.limit, f.one), (Some(5), Some(10), false));
        assert_eq!(find("db.users.find()"), Find::default());
        assert!(find("db.users.findOne({ _id: 1 })").one);
        assert_eq!(find("db.users.find().toArray()"), Find::default());
        assert!(
            matches!(op("db.users.find({a: 1}).count()"), CollectionOp::CountDocuments { filter, .. } if filter == doc! {"a": 1})
        );
        assert!(parse("db.users.find().nope()").is_err());
    }

    #[test]
    fn names_collections() {
        let collection = |src: &str| match parse(src).unwrap() {
            Command::Collection { db, collection, .. } => (db, collection),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            collection("db.system.views.find()"),
            (None, "system.views".into())
        );
        assert_eq!(collection("db['my coll'].find()"), (None, "my coll".into()));
        assert_eq!(
            collection("db.getCollection('a-b').find()"),
            (None, "a-b".into())
        );
        assert_eq!(
            collection("db.getSiblingDB('x').getCollection('y').find()"),
            (Some("x".into()), "y".into())
        );
        assert!(parse("db.users").is_err());
    }

    #[test]
    fn parses_writes() {
        assert!(
            matches!(op("db.c.insertOne({ a: 1 })"), CollectionOp::InsertOne(d) if d == doc! {"a": 1})
        );
        assert!(
            matches!(op("db.c.insertMany([{ a: 1 }, { a: 2 },])"), CollectionOp::InsertMany(d) if d.len() == 2)
        );
        assert!(parse("db.c.insertMany({ a: 1 })").is_err());
        assert_eq!(
            op("db.c.updateMany({}, { $set: { a: 1 } }, { upsert: true })"),
            CollectionOp::Update {
                filter: doc! {},
                update: Bson::Document(doc! { "$set": { "a": 1 } }),
                many: true,
                upsert: true,
                options: doc! {},
            }
        );
        assert!(matches!(
            op("db.c.updateOne({}, [{ $set: { a: 1 } }])"),
            CollectionOp::Update {
                update: Bson::Array(_),
                many: false,
                ..
            }
        ));
        assert_eq!(
            op("db.c.deleteOne({ a: 1 })"),
            CollectionOp::Delete {
                filter: doc! {"a": 1},
                many: false,
                options: doc! {},
            }
        );
        assert_eq!(
            op("db.c.deleteMany({})"),
            CollectionOp::Delete {
                filter: doc! {},
                many: true,
                options: doc! {},
            }
        );
        assert!(parse("db.c.deleteMany()").is_err());
        assert_eq!(
            op("db.c.createIndex({ a: 1 }, { unique: true })"),
            CollectionOp::CreateIndex {
                keys: doc! {"a": 1},
                options: doc! {"unique": true}
            }
        );
        assert_eq!(op("db.c.drop()"), CollectionOp::Drop);
        assert_eq!(
            op("db.c.renameCollection('d', true)"),
            CollectionOp::Rename {
                to: "d".into(),
                drop_target: true
            }
        );
    }

    #[test]
    fn parses_aggregates_and_counts() {
        assert_eq!(
            op(
                "db.c.aggregate([{ $match: { a: 1 } }, { $group: { _id: '$b', n: { $sum: 1 } } }]).toArray()"
            ),
            CollectionOp::Aggregate {
                pipeline: vec![
                    doc! {"$match": {"a": 1}},
                    doc! {"$group": {"_id": "$b", "n": {"$sum": 1}}}
                ],
                options: doc! {}
            }
        );
        assert_eq!(
            op("db.c.distinct('b', { a: 1 })"),
            CollectionOp::Distinct {
                field: "b".into(),
                filter: doc! {"a": 1},
                options: doc! {},
            }
        );
        assert_eq!(
            op("db.c.estimatedDocumentCount()"),
            CollectionOp::EstimatedDocumentCount
        );
        assert!(parse("db.c.aggregate({})").is_err());
    }

    #[test]
    fn classifies_reads() {
        let read = |src: &str| is_read_only(&parse(src).unwrap());
        assert!(read("db.c.find()"));
        assert!(read("db.c.aggregate([{ $match: {} }])"));
        assert!(!read("db.c.aggregate([{ $out: 'x' }])"));
        assert!(!read("db.c.deleteMany({})"));
        assert!(!read("db.runCommand({ drop: 'c' })"));
        assert!(!read("db.c.renameCollection('d')"));
        let kind_of = |src: &str| kind(&parse(src).unwrap());
        assert_eq!(kind_of("show dbs"), StatementKind::Query);
        assert_eq!(kind_of("db.c.insertOne({})"), StatementKind::Dml);
        assert_eq!(kind_of("db.c.createIndex({ a: 1 })"), StatementKind::Ddl);
        assert_eq!(kind_of("db.runCommand({ ping: 1 })"), StatementKind::Other);
        let all = |src: &str| {
            unfiltered(&parse(src).unwrap()).map(|(d, c)| (d.map(str::to_owned), c.to_owned()))
        };
        assert_eq!(all("db.c.deleteMany({})"), Some((None, "c".to_owned())));
        assert_eq!(
            all("db.getSiblingDB('x').c.updateMany({}, { $set: { a: 1 } })"),
            Some((Some("x".to_owned()), "c".to_owned()))
        );
        assert_eq!(all("db.c.deleteMany({ a: 1 })"), None);
        assert_eq!(all("db.c.deleteOne({})"), None);
    }

    #[test]
    fn parses_bulk_write() {
        let parsed = op("db.c.bulkWrite([
            { insertOne: { document: { a: 1 } } },
            { updateOne: { filter: { a: 1 }, update: { $set: { b: 1 } }, upsert: true } },
            { updateMany: { filter: { a: 2 }, update: [{ $set: { b: 2 } }] } },
            { replaceOne: { filter: { a: 3 }, replacement: { c: 3 } } },
            { deleteOne: { filter: { a: 4 } } },
            { deleteMany: { filter: { a: 5 } } },
        ], { ordered: false })");
        let CollectionOp::BulkWrite { ops, ordered, .. } = parsed else {
            panic!("not a bulkWrite: {parsed:?}");
        };
        assert!(!ordered);
        assert_eq!(ops[0], CollectionOp::InsertOne(doc! { "a": 1 }));
        assert!(matches!(
            &ops[1],
            CollectionOp::Update {
                many: false,
                upsert: true,
                ..
            }
        ));
        assert!(matches!(
            &ops[2],
            CollectionOp::Update {
                many: true,
                upsert: false,
                update: Bson::Array(_),
                ..
            }
        ));
        assert!(matches!(
            &ops[3],
            CollectionOp::Replace { upsert: false, .. }
        ));
        assert!(matches!(&ops[4], CollectionOp::Delete { many: false, .. }));
        assert!(matches!(&ops[5], CollectionOp::Delete { many: true, .. }));
        assert!(matches!(
            op("db.c.bulkWrite([{ deleteOne: { filter: {} } }])"),
            CollectionOp::BulkWrite { ordered: true, .. }
        ));
    }

    #[test]
    fn refuses_what_bulk_write_would_not_carry_out() {
        let message = |src: &str| parse(src).unwrap_err().message;
        for bad in [
            "db.c.bulkWrite()",
            "db.c.bulkWrite({ deleteOne: { filter: {} } })",
            "db.c.bulkWrite([])",
            "db.c.bulkWrite([1])",
            "db.c.bulkWrite([{ drop: {} }])",
            "db.c.bulkWrite([{ deleteOne: 1 }])",
            "db.c.bulkWrite([{ deleteOne: {} }])",
            "db.c.bulkWrite([{ updateOne: { filter: {} } }])",
            "db.c.bulkWrite([{ insertOne: {} }])",
            "db.c.bulkWrite([{ replaceOne: { filter: {} } }])",
            "db.c.bulkWrite([{ deleteOne: { filter: {} }, deleteMany: { filter: {} } }])",
            "db.c.bulkWrite([{ updateOne: { filter: {}, update: {}, upsert: 1 } }])",
            "db.c.bulkWrite([{ deleteOne: { filter: {} } }], { ordered: 0 })",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        // Dropping these would change other documents than the ones asked for.
        let unknown = "db.c.bulkWrite([{ updateOne: { filter: {}, update: {}, sort: { a: 1 } } }])";
        assert!(
            message(unknown).contains("`sort` is not supported"),
            "{}",
            message(unknown)
        );
        let top = "db.c.bulkWrite([{ deleteOne: { filter: {} } }], { retryWrites: true })";
        assert!(message(top).contains("`retryWrites`"));
    }

    #[test]
    fn bulk_write_is_a_write_and_cannot_hide_a_change_to_every_document() {
        let command = |src: &str| parse(src).unwrap();
        let one = "db.c.bulkWrite([{ deleteOne: { filter: {} } }])";
        assert!(!is_read_only(&command(one)));
        assert_eq!(kind(&command(one)), StatementKind::Dml);
        assert!(unfiltered(&command(one)).is_none());
        for every in [
            "db.c.bulkWrite([{ deleteOne: { filter: { a: 1 } } }, { deleteMany: { filter: {} } }])",
            "db.c.bulkWrite([{ updateMany: { filter: {}, update: { $set: { a: 1 } } } }])",
        ] {
            assert_eq!(unfiltered(&command(every)), Some((None, "c")), "{every}");
        }
        assert!(
            unfiltered(&command(
                "db.c.bulkWrite([{ deleteMany: { filter: { a: 1 } } }])"
            ))
            .is_none()
        );
    }

    #[test]
    fn every_write_keeps_its_options_and_refuses_the_others_by_name() {
        let update = op(
            "db.c.updateMany({}, { $set: { 'a.$[e]': 1 } }, { arrayFilters: [{ e: 1 }], collation: { locale: 'it' }, hint: 'a_1', upsert: true })",
        );
        let CollectionOp::Update {
            options,
            upsert,
            many,
            ..
        } = update
        else {
            panic!("not an update");
        };
        assert!(upsert && many);
        assert_eq!(
            options.keys().map(String::as_str).collect::<Vec<_>>(),
            ["arrayFilters", "collation", "hint"]
        );
        // `multi` on `updateOne` used to change every match.
        for one in ["updateOne", "updateMany"] {
            let src = format!("db.c.{one}({{}}, {{ $set: {{ a: 1 }} }}, {{ multi: true }})");
            let message = parse(&src).unwrap_err().message;
            assert!(message.contains("`multi` is not supported"), "{message}");
        }
        assert!(matches!(
            op("db.c.update({}, { $set: { a: 1 } }, { multi: true })"),
            CollectionOp::Update { many: true, .. }
        ));
        assert!(matches!(
            op("db.c.update({}, { $set: { a: 1 } })"),
            CollectionOp::Update { many: false, .. }
        ));
        let delete = op("db.c.deleteMany({ a: 1 }, { collation: { locale: 'it' } })");
        assert!(
            matches!(&delete, CollectionOp::Delete { options, .. } if options.contains_key("collation"))
        );
        assert!(matches!(
            op("db.c.remove({ a: 1 }, { justOne: true })"),
            CollectionOp::Delete { many: false, options, .. } if options.is_empty()
        ));
        assert!(matches!(
            op("db.c.insertMany([{ a: 1 }], { ordered: false })"),
            CollectionOp::InsertWith { options, .. } if options.get_bool("ordered") == Ok(false)
        ));
        assert!(matches!(
            op("db.c.insertMany([{ a: 1 }])"),
            CollectionOp::InsertMany(_)
        ));
        assert!(
            matches!(op("db.c.insertOne({ a: 1 }, { writeConcern: { w: 1 } })"), CollectionOp::InsertWith { documents, .. } if documents.len() == 1)
        );
        assert!(matches!(
            op("db.c.bulkWrite([{ updateOne: { filter: {}, update: {}, arrayFilters: [{ e: 1 }], hint: 'a_1' } }], { writeConcern: { w: 1 } })"),
            CollectionOp::BulkWrite { ops, options, .. }
                if options.contains_key("writeConcern")
                && matches!(&ops[0], CollectionOp::Update { options, .. } if options.contains_key("arrayFilters"))
        ));

        for (bad, key) in [
            (
                "db.c.updateOne({}, { $set: { a: 1 } }, { nope: 1 })",
                "nope",
            ),
            (
                "db.c.replaceOne({}, { a: 1 }, { arrayFilters: [] })",
                "arrayFilters",
            ),
            ("db.c.deleteOne({}, { upsert: true })", "upsert"),
            ("db.c.insertOne({}, { upsert: true })", "upsert"),
            ("db.c.find({}, {}, { nope: 1 })", "nope"),
            ("db.c.aggregate([], { nope: 1 })", "nope"),
            ("db.c.countDocuments({}, { sort: {} })", "sort"),
            ("db.c.distinct('a', {}, { sort: {} })", "sort"),
            ("db.c.findOneAndDelete({}, { nope: 1 })", "nope"),
            ("db.aggregate([], { nope: 1 })", "nope"),
        ] {
            let message = parse(bad).unwrap_err().message;
            assert!(
                message.contains(&format!("`{key}` is not supported")),
                "{bad}: {message}"
            );
        }
        assert!(parse("db.c.updateOne({}, { $set: { a: 1 } }, { upsert: 'yes' })").is_err());
    }

    #[test]
    fn find_takes_options_and_every_cursor_method_that_changes_its_result() {
        let f = find(
            "db.c.find({ a: 1 }, null, { sort: { a: 1 }, skip: 2, limit: 3, collation: { locale: 'it' } })",
        );
        assert_eq!(f.sort, Some(doc! { "a": 1 }));
        assert_eq!((f.skip, f.limit), (Some(2), Some(3)));
        assert_eq!(f.options, doc! { "collation": { "locale": "it" } });
        let f = find(
            "db.c.find().collation({ locale: 'it' }).hint({ a: 1 }).min({ a: 1 }).max({ a: 9 }).maxTimeMS(5).allowDiskUse().noCursorTimeout().returnKey(false).showRecordId().readConcern('local').batchSize(2).comment('x')",
        );
        assert_eq!(
            f.options,
            doc! {
                "collation": { "locale": "it" },
                "hint": { "a": 1 },
                "min": { "a": 1 },
                "max": { "a": 9 },
                "maxTimeMS": 5,
                "allowDiskUse": true,
                "noCursorTimeout": true,
                "returnKey": false,
                "showRecordId": true,
                "readConcern": { "level": "local" },
            }
        );
        // A count keeps what can change it, and leaves what it does not take.
        assert!(matches!(
            op("db.c.find({ a: 1 }).hint('a_1').allowDiskUse().count()"),
            CollectionOp::CountDocuments { options, .. } if options == doc! { "hint": "a_1" }
        ));
        for js in ["forEach(1)", "map(1)", "next()", "hasNext()", "tailable()"] {
            let message = parse(&format!("db.c.find().{js}")).unwrap_err().message;
            assert!(message.contains("not JavaScript"), "{js}: {message}");
        }
        assert!(parse("db.c.find().nope()").is_err());
    }

    #[test]
    fn parses_find_one_and_and_find_and_modify() {
        assert!(matches!(
            op("db.c.findOneAndUpdate({ a: 1 }, { $set: { b: 1 } }, { returnNewDocument: true, sort: { a: 1 } })"),
            CollectionOp::FindOneAndUpdate { options, .. }
                if options.get_str("returnDocument") == Ok("after") && !options.contains_key("returnNewDocument")
        ));
        assert!(matches!(
            op("db.c.findOneAndUpdate({}, [{ $set: { b: 1 } }])"),
            CollectionOp::FindOneAndUpdate {
                update: Bson::Array(_),
                ..
            }
        ));
        assert!(matches!(
            op("db.c.findOneAndReplace({}, { b: 1 })"),
            CollectionOp::FindOneAndReplace { .. }
        ));
        assert!(matches!(
            op("db.c.findOneAndDelete({ a: 1 })"),
            CollectionOp::FindOneAndDelete { .. }
        ));
        assert!(parse("db.c.findOneAndUpdate({})").is_err());

        assert!(matches!(
            op("db.c.findAndModify({ query: { a: 1 }, update: { $set: { b: 1 } }, new: true, fields: { b: 1 }, sort: { a: 1 }, upsert: true })"),
            CollectionOp::FindOneAndUpdate { filter, options, .. }
                if filter == doc! { "a": 1 }
                && options.get_str("returnDocument") == Ok("after")
                && options.contains_key("projection")
                && options.get_bool("upsert") == Ok(true)
        ));
        assert!(matches!(
            op("db.c.findAndModify({ query: { a: 1 }, remove: true })"),
            CollectionOp::FindOneAndDelete { .. }
        ));
        assert!(parse("db.c.findAndModify({ query: {} })").is_err());
        assert!(parse("db.c.findAndModify({ remove: true, update: {} })").is_err());
    }

    #[test]
    fn what_a_new_method_does_decides_how_it_is_guarded() {
        let parsed = |src: &str| parse(src).unwrap();
        let reads = |src: &str| is_read_only(&parsed(src));
        for src in [
            "db.c.stats()",
            "db.c.latencyStats()",
            "db.c.dataSize()",
            "db.c.storageSize()",
            "db.c.totalIndexSize()",
            "db.c.totalSize()",
            "db.c.isCapped()",
            "db.c.exists()",
            "db.c.getName()",
            "db.c.getFullName()",
            "db.c.getIndexKeys()",
            "db.c.getIndexSpecs()",
            "db.getName()",
            "show profile",
            // A plan changes nothing, even of a write.
            "db.c.find().explain()",
            "db.c.explain('executionStats').find({ a: 1 }).sort({ a: 1 })",
            "db.c.explain().deleteMany({})",
            "db.c.explain().updateMany({}, { $set: { a: 1 } })",
            "db.c.aggregate([{ $match: {} }]).explain()",
        ] {
            assert!(reads(src), "{src} should read");
            assert_eq!(kind(&parsed(src)), StatementKind::Query, "{src}");
        }
        for src in [
            "db.c.findOneAndUpdate({}, { $set: { a: 1 } })",
            "db.c.findOneAndReplace({}, { a: 1 })",
            "db.c.findOneAndDelete({})",
            "db.c.findAndModify({ remove: true })",
            "db.c.insertMany([{ a: 1 }], { ordered: false })",
        ] {
            assert!(!reads(src), "{src} should write");
            assert_eq!(kind(&parsed(src)), StatementKind::Dml, "{src}");
        }
        for src in [
            "db.c.createIndexes([{ a: 1 }, { b: 1 }])",
            "db.c.ensureIndex({ a: 1 })",
            "db.c.dropIndexes()",
            "db.c.dropIndexes('a_1')",
            "db.c.hideIndex('a_1')",
            "db.c.unhideIndex({ a: 1 })",
            "db.c.convertToCapped(1024)",
            // Reads nothing it changes, but it locks the collection.
            "db.c.validate()",
        ] {
            assert!(!reads(src), "{src} should write");
            assert_eq!(kind(&parsed(src)), StatementKind::Ddl, "{src}");
        }
        // These can do anything the server allows, as `runCommand` can: never taken for a read.
        for src in [
            "db.serverStatus()",
            "db.createUser({ user: 'a', pwd: 'b', roles: [] })",
            "db.dropUser('a')",
            "db.killOp(1)",
            "db.currentOp()",
            "db.setProfilingLevel(1)",
            "db.aggregate([{ $currentOp: {} }])",
            "rs.status()",
            "sh.status()",
            "show users",
            "show logs",
        ] {
            assert!(!reads(src), "{src} should not be taken for a read");
            assert_eq!(kind(&parsed(src)), StatementKind::Other, "{src}");
        }
        // An explain of what writes stays a write when it could run its stages.
        assert!(!reads(
            "db.c.aggregate([{ $out: 'd' }]).explain('executionStats')"
        ));
        // Only a change to every document asks; one document does not.
        assert!(unfiltered(&parsed("db.c.findOneAndDelete({})")).is_none());
        assert!(unfiltered(&parsed("db.c.explain().deleteMany({})")).is_none());
    }

    #[test]
    fn parses_helpers_into_their_commands() {
        let run = |src: &str| match parse(src).unwrap() {
            Command::RunCommand { db, command } => (db, command),
            other => panic!("{src}: {other:?}"),
        };
        assert_eq!(run("db.serverStatus()").1, doc! { "serverStatus": 1 });
        assert_eq!(run("db.hello()").1, doc! { "hello": 1 });
        assert_eq!(run("db.isMaster()").1, doc! { "hello": 1 });
        assert_eq!(
            run("db.setProfilingLevel(1, { slowms: 5 })").1,
            doc! { "profile": 1_i64, "slowms": 5 }
        );
        assert_eq!(run("db.getProfilingStatus()").1, doc! { "profile": -1 });
        assert_eq!(
            run("db.currentOp({ active: true })"),
            (
                Some("admin".to_owned()),
                doc! { "currentOp": 1, "active": true }
            )
        );
        assert_eq!(
            run("db.killOp(7)"),
            (Some("admin".to_owned()), doc! { "killOp": 1, "op": 7 })
        );
        assert_eq!(run("db.getUser('a')").1, doc! { "usersInfo": "a" });
        assert_eq!(run("db.getUsers()").1, doc! { "usersInfo": 1 });
        assert_eq!(run("db.getRole('r')").1, doc! { "rolesInfo": "r" });
        assert_eq!(
            run("db.createUser({ user: 'a', pwd: 'b', roles: ['read'] })").1,
            doc! { "createUser": "a", "pwd": "b", "roles": ["read"] }
        );
        assert_eq!(
            run("db.updateUser('a', { roles: [] })").1,
            doc! { "updateUser": "a", "roles": [] }
        );
        assert_eq!(run("db.dropUser('a')").1, doc! { "dropUser": "a" });
        assert_eq!(
            run("db.grantRolesToUser('a', ['read'])").1,
            doc! { "grantRolesToUser": "a", "roles": ["read"] }
        );
        assert_eq!(
            run("db.createRole({ role: 'r', privileges: [], roles: [] })").1,
            doc! { "createRole": "r", "privileges": [], "roles": [] }
        );
        assert_eq!(
            run("db.getSiblingDB('x').serverStatus()").0,
            Some("x".to_owned())
        );
        assert_eq!(
            run("db.aggregate([{ $currentOp: {} }])").1,
            doc! { "aggregate": 1, "pipeline": [{ "$currentOp": {} }], "cursor": {} }
        );
        assert_eq!(
            run("rs.status()"),
            (Some("admin".to_owned()), doc! { "replSetGetStatus": 1 })
        );
        assert_eq!(run("rs.conf()").1, doc! { "replSetGetConfig": 1 });
        assert_eq!(run("sh.status()").1, doc! { "listShards": 1 });
        assert_eq!(run("show users").1, doc! { "usersInfo": 1 });
        assert_eq!(run("show roles").1, doc! { "rolesInfo": 1 });
        assert_eq!(
            run("show logs"),
            (Some("admin".to_owned()), doc! { "getLog": "*" })
        );
        assert_eq!(
            run("show log startupWarnings").1,
            doc! { "getLog": "startupWarnings" }
        );
        assert!(matches!(
            parse("db.getName()"),
            Ok(Command::DatabaseName { db: None })
        ));
        assert!(matches!(
            parse("show profile"),
            Ok(Command::Collection { collection, op: CollectionOp::Find(_), .. }) if collection == "system.profile"
        ));
        for bad in [
            "db.createUser({ pwd: 'b' })",
            "db.killOp()",
            "db.dropUser()",
            "rs.nope()",
            "sh.nope()",
            "rs.status(1)",
            "rs",
            "show nope",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn says_why_what_needs_javascript_or_the_connection_is_refused() {
        let message = |src: &str| parse(src).unwrap_err().message;
        assert!(message("db.c.watch()").contains("take over the connection"));
        assert!(message("db.watch()").contains("take over the connection"));
        assert!(message("db.c.mapReduce()").contains("use aggregate()"));
        assert!(message("db.c.initializeOrderedBulkOp()").contains("use bulkWrite()"));
    }

    #[test]
    fn parses_explain_both_ways() {
        let explained = |src: &str| match op(src) {
            CollectionOp::Explain { inner, verbosity } => (*inner, verbosity),
            other => panic!("{src}: {other:?}"),
        };
        let (inner, verbosity) =
            explained("db.c.find({ a: 1 }).sort({ a: 1 }).explain('executionStats')");
        assert!(matches!(inner, CollectionOp::Find(f) if f.sort.is_some()));
        assert_eq!(verbosity, "executionStats");
        let (inner, verbosity) = explained("db.c.explain().find({ a: 1 }).sort({ a: 1 })");
        assert!(matches!(inner, CollectionOp::Find(f) if f.sort.is_some()));
        assert_eq!(verbosity, "queryPlanner");
        assert_eq!(
            explained("db.c.find().explain(true)").1,
            "allPlansExecution"
        );
        assert_eq!(explained("db.c.find().explain(false)").1, "queryPlanner");
        assert!(matches!(
            explained("db.c.aggregate([]).explain()").0,
            CollectionOp::Aggregate { .. }
        ));
        assert!(matches!(
            explained("db.c.explain().countDocuments({})").0,
            CollectionOp::CountDocuments { .. }
        ));
        assert!(matches!(
            explained("db.c.explain().remove({ a: 1 })").0,
            CollectionOp::Delete { .. }
        ));
        for bad in [
            "db.c.explain()",
            "db.c.explain().drop()",
            "db.c.drop().explain()",
            "db.c.find().explain('nope')",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn neighbouring_inserts_go_in_one_round_trip_when_ordered() {
        let insert = |n: i32| CollectionOp::InsertOne(doc! { "n": n });
        let delete = CollectionOp::Delete {
            filter: doc! { "n": 1 },
            many: false,
            options: doc! {},
        };
        let ops = vec![insert(1), insert(2), insert(3), delete.clone(), insert(4)];
        assert_eq!(
            write_batches(ops.clone(), true),
            vec![
                (
                    CollectionOp::InsertMany(vec![
                        doc! { "n": 1 },
                        doc! { "n": 2 },
                        doc! { "n": 3 }
                    ]),
                    3
                ),
                (delete, 1),
                (insert(4), 1),
            ]
        );
        let alone = write_batches(ops, false);
        assert_eq!(alone.len(), 5);
        assert!(alone.iter().all(|(_, size)| *size == 1));
    }

    #[test]
    fn reports_error_offsets() {
        let e = parse("db.c.find({ a: })").unwrap_err();
        assert_eq!(e.offset, 15);
        let e = parse("db.c.find().limit('x')").unwrap_err();
        assert_eq!(e.offset, 11);
    }

    #[test]
    fn splits_scripts() {
        assert_eq!(
            texts("use shop\ndb.a.find({\n  x: 1\n})\n  .sort({ x: 1 })\n\ndb.b.drop(); show dbs"),
            [
                "use shop",
                "db.a.find({\n  x: 1\n})\n  .sort({ x: 1 })",
                "db.b.drop()",
                "show dbs"
            ]
        );
        assert_eq!(
            texts("db.a.find({ s: ';\\n' }) // c;\n"),
            ["db.a.find({ s: ';\\n' }) // c;"]
        );
        assert_eq!(
            texts("db.a.find({ r: /;[/]/ });db.b.find()"),
            ["db.a.find({ r: /;[/]/ })", "db.b.find()"]
        );
        assert_eq!(texts("// only a comment\n\n/* x */"), Vec::<&str>::new());
        assert_eq!(texts("  \n "), Vec::<&str>::new());
    }

    #[test]
    fn finds_the_statement_at_the_cursor() {
        let src = "db.a.find()\ndb.b.find();";
        assert_eq!(statement_at(src, 3), Some(0..11));
        assert_eq!(statement_at(src, 14), Some(12..23));
        assert_eq!(statement_at(src, src.len()), Some(12..23));
    }
}
