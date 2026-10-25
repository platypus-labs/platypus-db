//! The subset of the mongosh language the editor runs: `show dbs`, `use db`, the `db.*`
//! helpers and the collection methods with their cursor modifiers. A statement becomes a
//! [`Command`]; [`split`] cuts a script into statements.

use std::ops::Range;

use mongodb::bson::{Bson, Document};

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
    },
    EstimatedDocumentCount,
    Distinct {
        field: String,
        filter: Document,
    },
    InsertOne(Document),
    InsertMany(Vec<Document>),
    Update {
        filter: Document,
        /// An update document, or an aggregation pipeline.
        update: Bson,
        many: bool,
        upsert: bool,
    },
    Replace {
        filter: Document,
        replacement: Document,
        upsert: bool,
    },
    Delete {
        filter: Document,
        many: bool,
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
        _ => {
            r.pos = start;
            return r.error(format!(
                "unknown statement `{first}`: expected `db.`, `show` or `use`"
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
        _ => {
            r.pos = at;
            r.error("expected `dbs` or `collections`")
        }
    }
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
                Some(Bson::String(name)) => mongodb::bson::doc! { name: 1 },
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
                mongodb::bson::doc! { "viewOn": source, "pipeline": pipeline }
            } else {
                args.opt_doc("the options")?
            };
            Command::CreateCollection { db, name, options }
        }
        "dropDatabase" => Command::RunCommand {
            db,
            command: mongodb::bson::doc! { "dropDatabase": 1 },
        },
        "stats" => Command::RunCommand {
            db,
            command: mongodb::bson::doc! { "dbStats": 1 },
        },
        "version" => Command::RunCommand {
            db,
            command: mongodb::bson::doc! { "buildInfo": 1 },
        },
        name => return err(offset, format!("unknown method `db.{name}()`")),
    })
}

fn collection_op(method: Segment, modifiers: Vec<Segment>) -> ParseResult<CollectionOp> {
    let name = method.name.clone();
    let offset = method.offset;
    let modifiers = modifiers.into_iter();
    let op = match name.as_str() {
        "find" | "findOne" => {
            let mut args = Args::new(method, 2)?;
            let mut find = Find {
                filter: args.opt_doc("the filter")?,
                projection: Some(args.opt_doc("the projection")?).filter(|p| !p.is_empty()),
                one: name == "findOne",
                ..Find::default()
            };
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
            CollectionOp::Aggregate { pipeline, options }
        }
        "countDocuments" | "count" => {
            let mut args = Args::new(method, 2)?;
            let filter = args.opt_doc("the filter")?;
            let options = args.opt_doc("the options")?;
            let number = |key: &str| {
                options
                    .get(key)
                    .and_then(integer)
                    .and_then(|n| u64::try_from(n).ok())
            };
            CollectionOp::CountDocuments {
                skip: number("skip"),
                limit: number("limit"),
                filter,
            }
        }
        "estimatedDocumentCount" => {
            Args::new(method, 1)?;
            CollectionOp::EstimatedDocumentCount
        }
        "distinct" => {
            let mut args = Args::new(method, 2)?;
            let field = args.string("the field")?;
            CollectionOp::Distinct {
                field,
                filter: args.opt_doc("the filter")?,
            }
        }
        "insertOne" | "insert" | "insertMany" | "updateOne" | "updateMany" | "update"
        | "replaceOne" | "deleteOne" | "deleteMany" | "remove" => write_op(&name, method)?,
        "drop" => {
            Args::new(method, 0)?;
            CollectionOp::Drop
        }
        "createIndex" => {
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
        "getIndexes" | "getIndices" => {
            Args::new(method, 0)?;
            CollectionOp::GetIndexes
        }
        other => return err(offset, format!("unknown collection method `{other}()`")),
    };
    // `aggregate([…]).toArray()` and the like.
    for m in modifiers {
        if !matches!(m.name.as_str(), "toArray" | "pretty") {
            return err(
                m.offset,
                format!("`.{}()` cannot follow `{name}()`", m.name),
            );
        }
    }
    Ok(op)
}

/// The collection methods that change documents.
fn write_op(name: &str, method: Segment) -> ParseResult<CollectionOp> {
    let offset = method.offset;
    Ok(match name {
        "insertOne" | "insert" | "insertMany" => {
            let mut args = Args::new(method, 2)?;
            match args.next() {
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
            }
        }
        "updateOne" | "updateMany" | "update" => {
            let mut args = Args::new(method, 3)?;
            let filter = args.doc("the filter")?;
            let Some(update @ (Bson::Document(_) | Bson::Array(_))) = args.next() else {
                return err(offset, format!("{name}() expects an update document"));
            };
            let options = args.opt_doc("the options")?;
            CollectionOp::Update {
                filter,
                update,
                many: name == "updateMany" || options.get_bool("multi").unwrap_or(false),
                upsert: options.get_bool("upsert").unwrap_or(false),
            }
        }
        "replaceOne" => {
            let mut args = Args::new(method, 3)?;
            let filter = args.doc("the filter")?;
            let replacement = args.doc("the replacement")?;
            let options = args.opt_doc("the options")?;
            CollectionOp::Replace {
                filter,
                replacement,
                upsert: options.get_bool("upsert").unwrap_or(false),
            }
        }
        "deleteOne" | "deleteMany" | "remove" => {
            let mut args = Args::new(method, 2)?;
            let filter = args.doc("the filter")?;
            let options = args.opt_doc("the options")?;
            CollectionOp::Delete {
                filter,
                many: name == "deleteMany"
                    || (name == "remove" && !options.get_bool("justOne").unwrap_or(false)),
            }
        }
        _ => unreachable!("not a write method: {name}"),
    })
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
                return Ok(CollectionOp::CountDocuments {
                    filter: find.filter,
                    skip: find.skip,
                    limit: find.limit.and_then(|n| u64::try_from(n.abs()).ok()),
                });
            }
            "toArray" | "pretty" | "batchSize" | "maxTimeMS" | "comment" | "hint" => {}
            other => return err(offset, format!("unknown cursor method `.{other}()`")),
        }
    }
    Ok(CollectionOp::Find(find))
}

/// Whether a statement only reads, for the checks the editor makes before running it.
#[must_use]
pub fn is_read_only(command: &Command) -> bool {
    match command {
        Command::ShowDatabases
        | Command::ShowCollections
        | Command::Use(_)
        | Command::ListCollections { .. } => true,
        Command::RunCommand { .. } | Command::CreateCollection { .. } => false,
        Command::Collection { op, .. } => {
            matches!(
                op,
                CollectionOp::Find(_)
                    | CollectionOp::CountDocuments { .. }
                    | CollectionOp::EstimatedDocumentCount
                    | CollectionOp::Distinct { .. }
                    | CollectionOp::GetIndexes
            ) || matches!(op, CollectionOp::Aggregate { pipeline, .. }
            if !pipeline.iter().any(|s| s.contains_key("$out") || s.contains_key("$merge")))
        }
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
            b';' if depth == 0 => {
                push(&mut out, start, i);
                start = i + 1;
                last = b';';
                i += 1;
                continue;
            }
            b'\n'
                if depth == 0 && !continues(&src[i + 1..]) && !src[start..i].trim().is_empty() =>
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
        assert!(parse("show users").is_err());
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
                upsert: true
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
                many: false
            }
        );
        assert_eq!(
            op("db.c.deleteMany({})"),
            CollectionOp::Delete {
                filter: doc! {},
                many: true
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
                filter: doc! {"a": 1}
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
