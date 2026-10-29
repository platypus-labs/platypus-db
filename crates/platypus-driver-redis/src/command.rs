//! The editor's text: one Redis command per line, its arguments quoted as `redis-cli`
//! quotes them (`"a b\x00"`, `'it''s'` is not: `'it\'s'`). Lines starting with `#` or `--`
//! are comments. Besides the server's commands, the editor takes `BROWSE`, which reads the
//! keys of one type as rows (see [`Browse`]), and runs `SELECT`, `MULTI`, `EXEC` and
//! `DISCARD` on the client: the session keeps a connection per database.

use std::fmt::Write as _;
use std::ops::Range;

/// The type of a key, as `TYPE` and `SCAN … TYPE` name it, and the virtual table that lists
/// the keys of that type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyType {
    String,
    Hash,
    List,
    Set,
    Zset,
    /// A `RedisJSON` document (module `ReJSON`, built into Redis 8 and Redis Stack).
    Json,
    Stream,
}

impl KeyType {
    pub const ALL: [Self; 7] = [
        Self::String,
        Self::Hash,
        Self::List,
        Self::Set,
        Self::Zset,
        Self::Json,
        Self::Stream,
    ];

    /// The virtual table's name.
    #[must_use]
    pub const fn table(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Hash => "hash",
            Self::List => "list",
            Self::Set => "set",
            Self::Zset => "zset",
            Self::Json => "json",
            Self::Stream => "stream",
        }
    }

    /// The name `TYPE` answers and `SCAN … TYPE` takes.
    #[must_use]
    pub const fn redis_name(self) -> &'static str {
        match self {
            Self::Json => "ReJSON-RL",
            other => other.table(),
        }
    }

    /// The type of a virtual table, by its name in any case.
    #[must_use]
    pub fn from_table(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.table().eq_ignore_ascii_case(name))
    }

    /// The type `TYPE` answered.
    #[must_use]
    pub fn from_redis(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.redis_name() == name)
    }

    /// The columns of the virtual table and whether each is part of its key.
    #[must_use]
    pub const fn columns(self) -> &'static [(&'static str, bool)] {
        match self {
            Self::String | Self::Json => &[("key", true), ("value", false), ("ttl", false)],
            Self::Hash => &[("key", true), ("field", true), ("value", false)],
            Self::List => &[("key", true), ("index", true), ("value", false)],
            Self::Set => &[("key", true), ("member", true)],
            Self::Zset => &[("key", true), ("member", true), ("score", false)],
            Self::Stream => &[("key", true), ("id", true), ("fields", false)],
        }
    }
}

/// `BROWSE [db<n>.]<type> [MATCH <pattern>] [LIMIT <rows>]`: the keys of one type (found
/// with `SCAN … TYPE`, never `KEYS`) and their contents, a row per value, read as the grid
/// asks for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browse {
    /// The database; the session's when `None`.
    pub db: Option<i64>,
    pub kind: KeyType,
    /// A glob of the keys, as `SCAN … MATCH` takes it.
    pub pattern: Option<Vec<u8>>,
    /// The most rows to read.
    pub limit: Option<u64>,
}

/// A line of the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Browse(Browse),
    /// `SELECT n`: the session's database.
    Select(i64),
    /// `MULTI`: commands queue on the client until `EXEC`, which sends them as one
    /// transaction, or `DISCARD`.
    Multi,
    Exec,
    Discard,
    /// Any other command, sent as it is.
    Raw(Vec<Vec<u8>>),
}

/// What a command does, for the checks before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Reads only: what a read-only connection runs.
    Query,
    /// Writes keys.
    Dml,
    /// Empties whole databases (`FLUSHDB`, `FLUSHALL`).
    Ddl,
    /// Changes the server (`CONFIG SET`, `ACL SETUSER`, `SHUTDOWN`, …).
    Other,
}

/// The commands that turn the connection into one that only receives (subscriptions,
/// `MONITOR`), or change what the client expects of it.
const REFUSED: &[&str] = &[
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "UNSUBSCRIBE",
    "PUNSUBSCRIBE",
    "SUNSUBSCRIBE",
    "MONITOR",
    "SYNC",
    "PSYNC",
    "HELLO",
    "RESET",
    "QUIT",
    "AUTH",
    "WATCH",
    "UNWATCH",
];

/// Commands that only read, whatever their arguments.
const READS: &[&str] = &[
    "BITCOUNT",
    "BITFIELD_RO",
    "BITPOS",
    "DBSIZE",
    "DUMP",
    "ECHO",
    "EVALSHA_RO",
    "EVAL_RO",
    "EXISTS",
    "EXPIRETIME",
    "FCALL_RO",
    "GEODIST",
    "GEOHASH",
    "GEOPOS",
    "GEORADIUSBYMEMBER_RO",
    "GEORADIUS_RO",
    "GEOSEARCH",
    "GET",
    "GETBIT",
    "GETRANGE",
    "HEXISTS",
    "HEXPIRETIME",
    "HGET",
    "HGETALL",
    "HKEYS",
    "HLEN",
    "HMGET",
    "HPEXPIRETIME",
    "HPTTL",
    "HRANDFIELD",
    "HSCAN",
    "HSTRLEN",
    "HTTL",
    "HVALS",
    "INFO",
    "KEYS",
    "LASTSAVE",
    "LCS",
    "LINDEX",
    "LLEN",
    "LOLWUT",
    "LPOS",
    "LRANGE",
    "MGET",
    "PEXPIRETIME",
    "PFCOUNT",
    "PING",
    "PTTL",
    "RANDOMKEY",
    "ROLE",
    "SCAN",
    "SCARD",
    "SDIFF",
    "SINTER",
    "SINTERCARD",
    "SISMEMBER",
    "SMEMBERS",
    "SMISMEMBER",
    "SORT_RO",
    "SRANDMEMBER",
    "SSCAN",
    "STRLEN",
    "SUBSTR",
    "SUNION",
    "TIME",
    "TOUCH",
    "TTL",
    "TYPE",
    "XLEN",
    "XPENDING",
    "XRANGE",
    "XREAD",
    "XREVRANGE",
    "ZCARD",
    "ZCOUNT",
    "ZDIFF",
    "ZINTER",
    "ZINTERCARD",
    "ZLEXCOUNT",
    "ZMSCORE",
    "ZRANDMEMBER",
    "ZRANGE",
    "ZRANGEBYLEX",
    "ZRANGEBYSCORE",
    "ZRANK",
    "ZREVRANGE",
    "ZREVRANGEBYLEX",
    "ZREVRANGEBYSCORE",
    "ZREVRANK",
    "ZSCAN",
    "ZSCORE",
    "ZUNION",
    // RedisJSON, RediSearch, RedisTimeSeries and the probabilistic types.
    "JSON.ARRINDEX",
    "JSON.ARRLEN",
    "JSON.DEBUG",
    "JSON.GET",
    "JSON.MGET",
    "JSON.OBJKEYS",
    "JSON.OBJLEN",
    "JSON.RESP",
    "JSON.STRLEN",
    "JSON.TYPE",
    "FT._LIST",
    "FT.AGGREGATE",
    "FT.EXPLAIN",
    "FT.INFO",
    "FT.PROFILE",
    "FT.SEARCH",
    "FT.SPELLCHECK",
    "FT.TAGVALS",
    "TS.GET",
    "TS.INFO",
    "TS.MGET",
    "TS.MRANGE",
    "TS.MREVRANGE",
    "TS.QUERYINDEX",
    "TS.RANGE",
    "TS.REVRANGE",
    "BF.EXISTS",
    "BF.INFO",
    "BF.MEXISTS",
    "CF.COUNT",
    "CF.EXISTS",
    "CF.INFO",
    "CMS.INFO",
    "CMS.QUERY",
    "TOPK.INFO",
    "TOPK.LIST",
    "TOPK.QUERY",
];

/// Commands that read with some subcommands only, and those subcommands.
const READ_SUBCOMMANDS: &[(&str, &[&str])] = &[
    (
        "ACL",
        &["CAT", "GETUSER", "LIST", "USERS", "WHOAMI", "LOG", "DRYRUN"],
    ),
    (
        "CLIENT",
        &["GETNAME", "ID", "INFO", "LIST", "TRACKINGINFO", "GETREDIR"],
    ),
    (
        "CLUSTER",
        &[
            "INFO",
            "NODES",
            "SHARDS",
            "SLOTS",
            "MYID",
            "KEYSLOT",
            "COUNTKEYSINSLOT",
        ],
    ),
    (
        "COMMAND",
        &[
            "COUNT",
            "DOCS",
            "GETKEYS",
            "GETKEYSANDFLAGS",
            "INFO",
            "LIST",
        ],
    ),
    ("CONFIG", &["GET"]),
    ("FUNCTION", &["DUMP", "LIST", "STATS"]),
    (
        "LATENCY",
        &["DOCTOR", "GRAPH", "HISTOGRAM", "HISTORY", "LATEST"],
    ),
    ("MEMORY", &["DOCTOR", "MALLOC-STATS", "STATS", "USAGE"]),
    ("MODULE", &["LIST"]),
    ("OBJECT", &["ENCODING", "FREQ", "IDLETIME", "REFCOUNT"]),
    (
        "PUBSUB",
        &[
            "CHANNELS",
            "NUMPAT",
            "NUMSUB",
            "SHARDCHANNELS",
            "SHARDNUMSUB",
        ],
    ),
    ("SCRIPT", &["EXISTS"]),
    ("SLOWLOG", &["GET", "LEN"]),
    ("XINFO", &["CONSUMERS", "GROUPS", "STREAM"]),
];

/// Commands that change the server rather than keys.
const ADMIN: &[&str] = &[
    "ACL",
    "BGREWRITEAOF",
    "BGSAVE",
    "CLIENT",
    "CLUSTER",
    "CONFIG",
    "DEBUG",
    "FAILOVER",
    "FUNCTION",
    "LATENCY",
    "MEMORY",
    "MODULE",
    "REPLICAOF",
    "SAVE",
    "SCRIPT",
    "SHUTDOWN",
    "SLAVEOF",
    "SLOWLOG",
];

impl Command {
    /// What the command does.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Browse(_) | Self::Select(_) | Self::Multi | Self::Exec | Self::Discard => {
                Kind::Query
            }
            Self::Raw(args) => raw_kind(args),
        }
    }

    /// The name of a raw command, upper case.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Browse(_) => "BROWSE".to_owned(),
            Self::Select(_) => "SELECT".to_owned(),
            Self::Multi => "MULTI".to_owned(),
            Self::Exec => "EXEC".to_owned(),
            Self::Discard => "DISCARD".to_owned(),
            Self::Raw(args) => upper(args.first().map_or(&[][..], Vec::as_slice)),
        }
    }
}

fn upper(arg: &[u8]) -> String {
    String::from_utf8_lossy(arg).to_ascii_uppercase()
}

fn raw_kind(args: &[Vec<u8>]) -> Kind {
    let name = upper(args.first().map_or(&[][..], Vec::as_slice));
    let sub = args.get(1).map(|a| upper(a)).unwrap_or_default();
    if READS.contains(&name.as_str()) {
        return Kind::Query;
    }
    if let Some((_, subs)) = READ_SUBCOMMANDS.iter().find(|(n, _)| *n == name)
        && subs.contains(&sub.as_str())
    {
        return Kind::Query;
    }
    // `SORT` without `STORE` reads; `XREAD` reads (but `XREADGROUP` moves a cursor).
    if name == "SORT" && !args.iter().skip(2).any(|a| upper(a) == "STORE") {
        return Kind::Query;
    }
    match name.as_str() {
        "FLUSHDB" | "FLUSHALL" => Kind::Ddl,
        n if ADMIN.contains(&n) => Kind::Other,
        _ => Kind::Dml,
    }
}

/// The commands of `src`: its lines that are not blank nor comments, without their outer
/// whitespace.
#[must_use]
pub fn split(src: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for line in src.split_inclusive('\n') {
        let end = start + line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let lead = text.len() - text.trim_start().len();
        let trimmed = text.trim();
        if !trimmed.is_empty() && !is_comment(trimmed) {
            out.push(start + lead..start + lead + trimmed.len());
        }
        start = end;
    }
    out
}

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with("--")
}

/// The command on the line of byte offset `at`, or else the nearest one above it.
#[must_use]
pub fn statement_at(src: &str, at: usize) -> Option<Range<usize>> {
    let line_start = src[..at.min(src.len())].rfind('\n').map_or(0, |i| i + 1);
    split(src)
        .into_iter()
        .take_while(|r| r.start <= at.max(line_start))
        .last()
}

/// Reads one line as `redis-cli` does: arguments split by spaces, `"…"` with backslash
/// escapes (`\n`, `\r`, `\t`, `\b`, `\a`, `\\`, `\"`, `\xHH`), `'…'` with `\'` only.
pub fn tokenize(line: &str) -> Result<Vec<Vec<u8>>, String> {
    let bytes = line.as_bytes();
    let mut args = Vec::new();
    let mut i = 0;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return Ok(args);
        }
        let mut arg = Vec::new();
        match bytes[i] {
            b'"' => i = double_quoted(bytes, i + 1, &mut arg)?,
            b'\'' => i = single_quoted(bytes, i + 1, &mut arg)?,
            _ => {
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    if matches!(bytes[i], b'"' | b'\'') {
                        return Err(format!("unexpected quote at column {}", i + 1));
                    }
                    arg.push(bytes[i]);
                    i += 1;
                }
            }
        }
        args.push(arg);
    }
}

/// After the closing quote at `i - 1`: the end of the argument.
fn after_quote(bytes: &[u8], i: usize) -> Result<usize, String> {
    match bytes.get(i) {
        None => Ok(i),
        Some(b) if b.is_ascii_whitespace() => Ok(i),
        Some(_) => Err(format!(
            "a closing quote must be followed by a space (column {})",
            i + 1
        )),
    }
}

fn double_quoted(bytes: &[u8], mut i: usize, arg: &mut Vec<u8>) -> Result<usize, String> {
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return after_quote(bytes, i + 1),
            b'\\' if i + 1 < bytes.len() => {
                let c = bytes[i + 1];
                let hex = |b: u8| char::from(b).to_digit(16);
                if c == b'x'
                    && let (Some(h), Some(l)) = (
                        bytes.get(i + 2).copied().and_then(hex),
                        bytes.get(i + 3).copied().and_then(hex),
                    )
                {
                    #[expect(clippy::cast_possible_truncation, reason = "two hex digits")]
                    arg.push((h * 16 + l) as u8);
                    i += 4;
                    continue;
                }
                arg.push(match c {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 8,
                    b'a' => 7,
                    other => other,
                });
                i += 2;
            }
            b => {
                arg.push(b);
                i += 1;
            }
        }
    }
    Err("unbalanced quotes".to_owned())
}

fn single_quoted(bytes: &[u8], mut i: usize, arg: &mut Vec<u8>) -> Result<usize, String> {
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => return after_quote(bytes, i + 1),
            b'\\' if bytes.get(i + 1) == Some(&b'\'') => {
                arg.push(b'\'');
                i += 2;
            }
            b => {
                arg.push(b);
                i += 1;
            }
        }
    }
    Err("unbalanced quotes".to_owned())
}

/// An argument as [`tokenize`] reads it back: as it is when nothing in it needs quoting,
/// else in double quotes with escapes.
#[must_use]
pub fn quote(arg: &[u8]) -> String {
    let plain = !arg.is_empty()
        && std::str::from_utf8(arg).is_ok()
        && !arg
            .iter()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control() || matches!(b, b'"' | b'\''));
    if plain {
        return String::from_utf8_lossy(arg).into_owned();
    }
    escaped(arg)
}

/// `"…"` with every byte that is not printable UTF-8 escaped.
fn escaped(arg: &[u8]) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for chunk in arg.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() && c.is_ascii() => {
                    let _ = write!(out, "\\x{:02x}", u32::from(c));
                }
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('"');
    out
}

/// A value as the grid shows it: its text when it is UTF-8, else quoted with escapes as
/// [`quote`] writes it (`"\xff\x00"`), which [`cell_bytes`] reads back.
#[must_use]
pub fn display(value: &[u8]) -> String {
    match std::str::from_utf8(value) {
        Ok(text) => text.to_owned(),
        Err(_) => escaped(value),
    }
}

/// The bytes of a cell's text: what [`display`] shows for a value that is not UTF-8 is read
/// back to its bytes, other text is its UTF-8.
#[must_use]
pub fn cell_bytes(text: &str) -> Vec<u8> {
    if text.len() >= 2
        && text.starts_with('"')
        && text.ends_with('"')
        && let Ok(mut args) = tokenize(text)
        && args.len() == 1
        && let Some(bytes) = args.pop()
        && std::str::from_utf8(&bytes).is_err()
    {
        return bytes;
    }
    text.as_bytes().to_vec()
}

/// Parses one line of the editor.
pub fn parse(line: &str) -> Result<Command, String> {
    let args = tokenize(line)?;
    let Some(first) = args.first() else {
        return Err("empty command".to_owned());
    };
    let name = upper(first);
    if REFUSED.contains(&name.as_str()) {
        return Err(format!(
            "{name} is not available in the editor: it would take over the connection"
        ));
    }
    let arity = |n: usize| {
        if args.len() == n {
            Ok(())
        } else {
            Err(format!("wrong number of arguments for '{name}'"))
        }
    };
    match name.as_str() {
        "BROWSE" => browse(&args[1..]).map(Command::Browse),
        "SELECT" => {
            arity(2)?;
            database(&args[1]).map(Command::Select)
        }
        "MULTI" => arity(1).map(|()| Command::Multi),
        "EXEC" => arity(1).map(|()| Command::Exec),
        "DISCARD" => arity(1).map(|()| Command::Discard),
        _ => Ok(Command::Raw(args)),
    }
}

/// A database number: `3` or `db3`.
fn database(arg: &[u8]) -> Result<i64, String> {
    let text = String::from_utf8_lossy(arg);
    let digits = text.strip_prefix("db").unwrap_or(&text);
    digits
        .parse::<i64>()
        .ok()
        .filter(|n| *n >= 0)
        .ok_or_else(|| format!("invalid database `{text}`"))
}

fn browse(args: &[Vec<u8>]) -> Result<Browse, String> {
    const USAGE: &str = "usage: BROWSE [db<n>.]<type> [MATCH <pattern>] [LIMIT <rows>]";
    let Some(target) = args.first() else {
        return Err(USAGE.to_owned());
    };
    let target = String::from_utf8_lossy(target);
    let (db, table) = match target.split_once('.') {
        Some((db, table)) => (Some(database(db.as_bytes())?), table),
        None => (None, &*target),
    };
    let kind = KeyType::from_table(table).ok_or_else(|| {
        format!("unknown type `{table}`: string, hash, list, set, zset, json or stream")
    })?;
    let mut out = Browse {
        db,
        kind,
        pattern: None,
        limit: None,
    };
    let mut rest = args[1..].iter();
    while let Some(word) = rest.next() {
        let value = rest.next().ok_or_else(|| USAGE.to_owned())?;
        match upper(word).as_str() {
            "MATCH" => out.pattern = Some(value.clone()),
            "LIMIT" => {
                let n = String::from_utf8_lossy(value);
                out.limit = Some(n.parse().map_err(|_| format!("invalid LIMIT `{n}`"))?);
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(out)
}

impl Browse {
    /// The command as the editor writes it.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::from("BROWSE ");
        if let Some(db) = self.db {
            let _ = write!(out, "db{db}.");
        }
        out.push_str(self.kind.table());
        if let Some(pattern) = &self.pattern {
            let _ = write!(out, " MATCH {}", quote(pattern));
        }
        if let Some(limit) = self.limit {
            let _ = write!(out, " LIMIT {limit}");
        }
        out
    }
}

/// A command line of `args`, each quoted as needed.
#[must_use]
pub fn line<A: AsRef<[u8]>>(args: &[A]) -> String {
    args.iter()
        .map(|a| quote(a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        tokenize(line)
            .unwrap()
            .into_iter()
            .map(|a| String::from_utf8(a).unwrap())
            .collect()
    }

    #[test]
    fn tokenizes_like_redis_cli() {
        assert_eq!(args("SET a  b"), ["SET", "a", "b"]);
        assert_eq!(args(r#"SET "a b" 'c\'d'"#), ["SET", "a b", "c'd"]);
        assert_eq!(args(r#"SET k "x\ny\t\"z\\""#), ["SET", "k", "x\ny\t\"z\\"]);
        assert_eq!(tokenize(r#"SET k "\xff\x00""#).unwrap()[2], [0xff, 0]);
        assert_eq!(args(r#"SET k """#), ["SET", "k", ""]);
        assert!(tokenize(r#"SET "a"b"#).is_err());
        assert!(tokenize(r#"SET "a"#).is_err());
        assert!(tokenize("SET a'b").is_err());
        assert_eq!(tokenize("").unwrap(), [] as [std::vec::Vec<u8>; 0]);
    }

    #[test]
    fn quoting_round_trips() {
        for value in [
            &b"plain"[..],
            b"",
            b"a b",
            b"it's",
            b"say \"hi\"",
            b"back\\slash \x00",
            b"\xff\xfe\n",
            "caff\u{e8}".as_bytes(),
        ] {
            let line = format!("SET {}", quote(value));
            assert_eq!(tokenize(&line).unwrap()[1], value, "{line}");
        }
        assert_eq!(quote(b"plain"), "plain");
        assert_eq!(quote(b""), r#""""#);
        assert_eq!(quote(b"\xff"), r#""\xff""#);
    }

    #[test]
    fn binary_cells_read_back() {
        assert_eq!(display(b"text"), "text");
        let shown = display(b"\xff\x00a");
        assert_eq!(shown, r#""\xff\x00a""#);
        assert_eq!(cell_bytes(&shown), b"\xff\x00a");
        // Quoted text that is UTF-8 stays text.
        assert_eq!(cell_bytes(r#""quoted""#), br#""quoted""#);
        assert_eq!(cell_bytes("a\\x41"), b"a\\x41");
    }

    #[test]
    fn splits_lines_and_skips_comments() {
        let src = "GET a\n\n  # note\n  SET b 1  \r\n-- x\nDEL c";
        let lines: Vec<_> = split(src).into_iter().map(|r| &src[r]).collect();
        assert_eq!(lines, ["GET a", "SET b 1", "DEL c"]);
        assert_eq!(statement_at(src, 2).map(|r| &src[r]), Some("GET a"));
        assert_eq!(statement_at(src, 7).map(|r| &src[r]), Some("GET a"));
        assert_eq!(statement_at(src, src.len()).map(|r| &src[r]), Some("DEL c"));
    }

    #[test]
    fn classifies_commands() {
        let kind = |line| parse(line).unwrap().kind();
        assert_eq!(kind("get a"), Kind::Query);
        assert_eq!(kind("HGETALL h"), Kind::Query);
        assert_eq!(kind("CONFIG GET maxmemory"), Kind::Query);
        assert_eq!(kind("CONFIG SET maxmemory 1"), Kind::Other);
        assert_eq!(kind("SORT l"), Kind::Query);
        assert_eq!(kind("SORT l STORE d"), Kind::Dml);
        assert_eq!(kind("SET a 1"), Kind::Dml);
        assert_eq!(kind("UNKNOWNCMD a"), Kind::Dml);
        assert_eq!(kind("FLUSHDB"), Kind::Ddl);
        assert_eq!(kind("BROWSE hash"), Kind::Query);
        assert_eq!(kind("SELECT 2"), Kind::Query);
        assert!(parse("SUBSCRIBE ch").is_err());
        assert!(parse("SELECT").is_err());
        assert!(parse("SELECT x").is_err());
    }

    #[test]
    fn parses_browse() {
        let b = |line| match parse(line).unwrap() {
            Command::Browse(b) => b,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            b("browse db3.Hash match 'user:*' limit 10"),
            Browse {
                db: Some(3),
                kind: KeyType::Hash,
                pattern: Some(b"user:*".to_vec()),
                limit: Some(10),
            }
        );
        assert_eq!(b("BROWSE zset").db, None);
        assert!(parse("BROWSE").is_err());
        assert!(parse("BROWSE nope").is_err());
        assert!(parse("BROWSE hash MATCH").is_err());
        assert!(parse("BROWSE hash LIMIT x").is_err());
        let text = b("BROWSE db1.set MATCH \"a b\" LIMIT 5").text();
        assert_eq!(text, "BROWSE db1.set MATCH \"a b\" LIMIT 5");
        assert_eq!(b(&text).pattern.as_deref(), Some(&b"a b"[..]));
    }
}
