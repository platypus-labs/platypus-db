//! Migration status of a project folder: which migrations of Flyway, Liquibase, Prisma, Alembic
//! and Rails are applied to the database, which are pending, and which differ from what was
//! applied. [`detect`] reads the folder, [`evaluate`] matches its files against the rows of a
//! tool's history table, and [`status`] reads those rows through a connection. The first two are
//! pure so that they are tested with files in a temporary folder.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

use platypus_core::{Connection, Error, ObjectKind, Result, TextRow, TextRowExt, query_text};
use platypus_sql::Dialect;
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Folders that hold dependencies or build output, never a project's own migrations.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "venv",
    ".venv",
    "build",
    "dist",
    "out",
    "__pycache__",
    ".gradle",
    ".idea",
    "vendor",
    "site-packages",
    ".tox",
    ".bundle",
];
/// How deep below the chosen folder migrations are looked for.
const MAX_DEPTH: usize = 6;
/// Files above this size are not migrations; they are skipped rather than read whole.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// A project with more files than this is not walked further.
const MAX_FILES: usize = 50_000;

/// A migration tool whose files and history table are understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Flyway,
    Liquibase,
    Prisma,
    Alembic,
    Rails,
}

impl Tool {
    const ALL: [Tool; 5] = [
        Tool::Flyway,
        Tool::Liquibase,
        Tool::Prisma,
        Tool::Alembic,
        Tool::Rails,
    ];

    /// The default name of the table where the tool records what it applied.
    fn history_table(self) -> &'static str {
        match self {
            Tool::Flyway => "flyway_schema_history",
            Tool::Liquibase => "DATABASECHANGELOG",
            Tool::Prisma => "_prisma_migrations",
            Tool::Alembic => "alembic_version",
            Tool::Rails => "schema_migrations",
        }
    }
}

/// Where a migration stands between the folder and the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Applied,
    /// In the folder, not applied yet.
    Pending,
    /// Applied, but the file's checksum is not the one the database recorded.
    Divergent,
    /// The tool recorded the run as failed.
    Failed,
    /// Recorded in the database, with no file in the folder.
    Missing,
}

/// One migration, as the folder and the database say it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Migration {
    pub tool: Tool,
    /// The version, revision, name or changeset id; `R` for a Flyway repeatable migration.
    pub id: String,
    pub description: String,
    /// The file relative to the chosen folder, with `/` separators; `None` when missing.
    pub file: Option<String>,
    pub status: Status,
    /// When the database says it was applied, as it prints it.
    pub applied_at: Option<String>,
    /// Why the status is what it is (a checksum pair, `out of order`, ...).
    pub detail: Option<String>,
}

/// What [`status`] found for one tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolStatus {
    pub tool: Tool,
    /// The history table as the database names it; `None` when it does not exist, which means
    /// nothing was applied.
    pub history_table: Option<String>,
    pub migrations: Vec<Migration>,
}

/// A migration read from a file of the folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Local {
    /// The version, revision, name or changeset id; empty for a Flyway repeatable migration.
    pub id: String,
    pub description: String,
    /// Relative to the chosen folder, with `/` separators.
    pub file: String,
    /// What the tool compares to detect an edited file: Flyway's CRC32 as a signed number,
    /// Prisma's SHA-256 in hex. `None` where the tool's checksum is not computed.
    pub checksum: Option<String>,
    /// A Liquibase changeset's author.
    pub author: Option<String>,
    /// The revisions an Alembic revision follows.
    pub parents: Vec<String>,
}

/// The migrations of one tool found in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub tool: Tool,
    pub files: Vec<Local>,
}

/// How a history table's row ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliedState {
    Done,
    Failed,
    /// Prisma's `migrate resolve --rolled-back`: the run no longer counts.
    RolledBack,
    /// A Flyway baseline: everything up to its version counts as applied.
    Baseline,
}

/// A row of a tool's history table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// As [`Local::id`].
    pub id: String,
    pub description: Option<String>,
    pub checksum: Option<String>,
    pub at: Option<String>,
    pub state: AppliedState,
    pub author: Option<String>,
    /// The file name the tool recorded.
    pub file: Option<String>,
    /// Anything the tool recorded about the run that is worth showing (Liquibase's exec type).
    pub note: Option<String>,
}

impl Applied {
    fn new(id: String, state: AppliedState) -> Self {
        Self {
            id,
            description: None,
            checksum: None,
            at: None,
            state,
            author: None,
            file: None,
            note: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------------------------

/// Looks for the migrations of every supported tool below `dir`, skipping dependency and
/// build folders. Tools with no migration in the folder are left out. Unreadable files and
/// folders are skipped: a half-readable project still shows what it can.
#[must_use]
pub fn detect(dir: &Path) -> Vec<Found> {
    let mut paths = Vec::new();
    walk(dir, 0, &mut paths);
    let mut found: HashMap<Tool, Vec<Local>> = HashMap::new();
    for path in &paths {
        let rel = relative(dir, path);
        for (tool, local) in classify(path, &rel) {
            found.entry(tool).or_default().push(local);
        }
    }
    Tool::ALL
        .into_iter()
        .filter_map(|tool| {
            let files = found.remove(&tool)?;
            Some(Found { tool, files })
        })
        .collect()
}

/// Every file below `dir` in a stable order. Symbolic links are not followed.
fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(std::result::Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            if depth < MAX_DEPTH && !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(&entry.path(), depth + 1, out);
            }
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The name of the folder `levels` above `path` (1 is the folder that holds the file).
fn ancestor_name(path: &Path, levels: usize) -> Option<String> {
    let dir = path.ancestors().nth(levels)?;
    Some(dir.file_name()?.to_string_lossy().into_owned())
}

/// What a file is a migration of, if anything. Only the files that can be one are read.
fn classify(path: &Path, rel: &str) -> Vec<(Tool, Local)> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let Some(name) = name else {
        return Vec::new();
    };
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let in_dir =
        |levels: usize, expected: &str| ancestor_name(path, levels).as_deref() == Some(expected);
    match ext.as_str() {
        "sql" => classify_sql(path, rel, &name),
        "xml" => read_text(path)
            .map(|text| liquibase(rel, changesets_xml(&text)))
            .unwrap_or_default(),
        "yaml" | "yml" => read_text(path)
            .map(|text| liquibase(rel, changesets_yaml(&text)))
            .unwrap_or_default(),
        "py" if in_dir(1, "versions") => read_text(path)
            .and_then(|text| alembic_revision(rel, &name, &text))
            .map(|l| vec![(Tool::Alembic, l)])
            .unwrap_or_default(),
        "rb" if in_dir(1, "migrate") && in_dir(2, "db") => rails_migration(rel, &name)
            .map(|l| vec![(Tool::Rails, l)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn classify_sql(path: &Path, rel: &str, name: &str) -> Vec<(Tool, Local)> {
    if let Some((version, description, repeatable)) = flyway_name(name) {
        let Ok(bytes) = std::fs::read(path) else {
            return Vec::new();
        };
        let local = Local {
            id: if repeatable { String::new() } else { version },
            description,
            file: rel.to_owned(),
            checksum: Some(flyway_checksum(&bytes).to_string()),
            ..Local::default()
        };
        return vec![(Tool::Flyway, local)];
    }
    if name == "migration.sql" && is_prisma_migration(path) {
        let Ok(bytes) = std::fs::read(path) else {
            return Vec::new();
        };
        let id = ancestor_name(path, 1).unwrap_or_default();
        let local = Local {
            description: id.clone(),
            id,
            file: rel.to_owned(),
            checksum: Some(prisma_checksum(&bytes)),
            ..Local::default()
        };
        return vec![(Tool::Prisma, local)];
    }
    match read_text(path) {
        Some(text) => liquibase(rel, changesets_sql(&text)),
        None => Vec::new(),
    }
}

/// `prisma/migrations/<name>/migration.sql`, or any `migrations/<name>/migration.sql` next to
/// the `migration_lock.toml` Prisma writes.
fn is_prisma_migration(path: &Path) -> bool {
    if ancestor_name(path, 2).as_deref() != Some("migrations") {
        return false;
    }
    ancestor_name(path, 3).as_deref() == Some("prisma")
        || path
            .ancestors()
            .nth(2)
            .is_some_and(|d| d.join("migration_lock.toml").is_file())
}

/// Splits a Flyway file name into its version, its description (underscores read as spaces,
/// like Flyway stores it) and whether it is repeatable. `None` for any other name.
fn flyway_name(name: &str) -> Option<(String, String, bool)> {
    let stem = name
        .strip_suffix(".sql")
        .or_else(|| name.strip_suffix(".SQL"))?;
    let (head, description) = stem.split_once("__")?;
    let description = description.replace('_', " ");
    if head == "R" {
        return Some((String::new(), description, true));
    }
    let version = head.strip_prefix('V')?;
    let valid = version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '_');
    valid.then(|| (version.replace('_', "."), description, false))
}

/// Flyway's checksum of a migration, as it stores it: a CRC32 over the bytes of every line
/// with its terminator removed (Flyway's `ChecksumCalculator` feeds each `readLine()` result to
/// the CRC and nothing between them), after the byte order mark of the first line is dropped.
/// Dropping every `\r` and `\n` is the same as reading by lines. The 32 bits are read as a
/// signed number because Flyway keeps an `int`.
#[must_use]
pub fn flyway_checksum(bytes: &[u8]) -> i32 {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut crc = crc32fast::Hasher::new();
    for part in text.split(['\r', '\n']) {
        crc.update(part.as_bytes());
    }
    // Reinterpreting the bits is the point: Flyway casts the CRC to `int`.
    #[allow(clippy::cast_possible_wrap)]
    let signed = crc.finalize() as i32;
    signed
}

/// Prisma's checksum of a migration: the SHA-256 of the file, in lower case hex.
#[must_use]
pub fn prisma_checksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

fn rails_migration(rel: &str, name: &str) -> Option<Local> {
    let stem = name.strip_suffix(".rb")?;
    let (version, description) = stem.split_once('_')?;
    let valid = version.len() == 14 && version.chars().all(|c| c.is_ascii_digit());
    valid.then(|| Local {
        id: version.to_owned(),
        description: description.replace('_', " "),
        file: rel.to_owned(),
        ..Local::default()
    })
}

/// A Liquibase changeset: its id and author.
type Changeset = (String, String);

fn liquibase(rel: &str, sets: Vec<Changeset>) -> Vec<(Tool, Local)> {
    sets.into_iter()
        .map(|(id, author)| {
            let local = Local {
                id,
                description: author.clone(),
                file: rel.to_owned(),
                author: Some(author),
                ..Local::default()
            };
            (Tool::Liquibase, local)
        })
        .collect()
}

fn changesets_xml(text: &str) -> Vec<Changeset> {
    use quick_xml::events::Event;
    if !text.contains("changeSet") {
        return Vec::new();
    }
    let mut reader = quick_xml::Reader::from_str(text);
    let mut sets = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "changeSet" => {
                let (mut id, mut author) = (None, None);
                for attr in e.attributes().flatten() {
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map(std::borrow::Cow::into_owned)
                        .ok();
                    match attr.key.local_name().as_ref() {
                        "id" => id = value,
                        "author" => author = value,
                        _ => {}
                    }
                }
                if let (Some(id), Some(author)) = (id, author) {
                    sets.push((id, author));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    sets
}

/// A tolerant reader of YAML changelogs: a `changeSet:` key opens a block, and the `id:` and
/// `author:` keys indented under it name the changeset. Flow style (`{id: 1, author: a}`)
/// is not read.
fn changesets_yaml(text: &str) -> Vec<Changeset> {
    let mut sets = Vec::new();
    // The indent of the open `changeSet:` key, and what it has named so far.
    let mut open: Option<(usize, Option<String>, Option<String>)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let key_line = trimmed.strip_prefix("- ").map_or(trimmed, str::trim_start);
        // `- ` shifts the key by two columns.
        let key_indent = indent + (trimmed.len() - key_line.len());
        if let Some((base, id, author)) = open.as_mut()
            && key_indent > *base
        {
            match key_line.split_once(':') {
                Some(("id", v)) if id.is_none() => *id = Some(yaml_scalar(v)),
                Some(("author", v)) if author.is_none() => *author = Some(yaml_scalar(v)),
                _ => {}
            }
            continue;
        }
        if let Some((_, Some(id), Some(author))) = open.take() {
            sets.push((id, author));
        }
        if key_line.trim_end() == "changeSet:" {
            open = Some((key_indent, None, None));
        }
    }
    if let Some((_, Some(id), Some(author))) = open {
        sets.push((id, author));
    }
    sets
}

fn yaml_scalar(value: &str) -> String {
    let value = value.trim();
    let unquoted = [('"', '"'), ('\'', '\'')]
        .into_iter()
        .find_map(|(a, b)| value.strip_prefix(a)?.strip_suffix(b));
    unquoted.unwrap_or(value).to_owned()
}

/// The changesets of a Liquibase "formatted SQL" changelog: `-- changeset author:id`.
fn changesets_sql(text: &str) -> Vec<Changeset> {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let header = first.trim().to_ascii_lowercase().replace("-- ", "--");
    if !header.starts_with("--liquibase formatted sql") {
        return Vec::new();
    }
    text.lines()
        .filter_map(|line| {
            let line = line.trim().replace("-- ", "--");
            let rest = line
                .get(..11)
                .filter(|p| p.eq_ignore_ascii_case("--changeset"))
                .and_then(|_| line.get(11..))?;
            let token = rest.split_whitespace().next()?;
            let (author, id) = token.split_once(':')?;
            Some((id.to_owned(), author.to_owned()))
        })
        .collect()
}

/// The revision an Alembic version file declares, read as text because running the file would
/// run the project's Python.
fn alembic_revision(rel: &str, name: &str, text: &str) -> Option<Local> {
    let assign = |key: &str| {
        // `revision = "x"` and the annotated `revision: str = "x"` of newer templates.
        Regex::new(&format!(r"(?m)^{key}\s*(?::[^=\n]+)?=\s*(.*)$")).ok()
    };
    let revision = assign("revision")?;
    let rhs = revision.captures(text)?.get(1)?.as_str();
    let id = quoted(rhs).into_iter().next()?;
    let down = assign("down_revision")?;
    let down = down
        .captures(text)
        .and_then(|c| c.get(1))
        .map_or(Vec::new(), |m| {
            multi_line_value(text, m.start(), m.as_str())
        });
    let description = docstring_title(text)
        .or_else(|| {
            let stem = name.strip_suffix(".py")?;
            stem.split_once('_').map(|(_, rest)| rest.replace('_', " "))
        })
        .unwrap_or_default();
    Some(Local {
        id,
        description,
        file: rel.to_owned(),
        parents: down.iter().flat_map(|l| quoted(l)).collect(),
        ..Local::default()
    })
}

/// The value that starts at `start` in `text`, joined over the lines of a bracketed value
/// (`down_revision = (\n "a",\n "b",\n)`).
fn multi_line_value(text: &str, start: usize, first: &str) -> Vec<String> {
    let opens = |s: &str| s.matches(['(', '[']).count();
    let closes = |s: &str| s.matches([')', ']']).count();
    if opens(first) <= closes(first) {
        return vec![first.to_owned()];
    }
    let mut lines = vec![first.to_owned()];
    let (mut open, mut close) = (opens(first), closes(first));
    for line in text[start + first.len()..].lines().skip(1) {
        lines.push(line.to_owned());
        open += opens(line);
        close += closes(line);
        if close >= open {
            break;
        }
    }
    lines
}

/// The quoted strings of a Python expression: `'a'`, `("a", "b")`, `None` (nothing).
fn quoted(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = expr.chars();
    while let Some(c) = chars.next() {
        if c == '#' {
            break;
        }
        if c == '\'' || c == '"' {
            let value: String = chars.by_ref().take_while(|&d| d != c).collect();
            out.push(value);
        }
    }
    out
}

/// The first line of a module docstring, which is Alembic's migration message.
fn docstring_title(text: &str) -> Option<String> {
    let start = text.find("\"\"\"")? + 3;
    let line = text[start..]
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    let line = line.trim_end_matches('"').trim();
    (!line.is_empty()).then(|| line.to_owned())
}

// ---------------------------------------------------------------------------------------------
// Matching files with history rows
// ---------------------------------------------------------------------------------------------

/// The status of every migration of `tool`: the files of the folder matched with the rows of
/// the history table, then the rows that have no file.
#[must_use]
pub fn evaluate(tool: Tool, local: &[Local], applied: &[Applied]) -> Vec<Migration> {
    match tool {
        Tool::Flyway => flyway(local, applied),
        Tool::Alembic => alembic(local, applied),
        Tool::Liquibase => keyed(
            tool,
            local,
            applied,
            |l| liquibase_key(&l.id, l.author.as_deref(), Some(&l.file)),
            |a| liquibase_key(&a.id, a.author.as_deref(), a.file.as_deref()),
        ),
        Tool::Prisma | Tool::Rails => {
            let mut all = keyed(tool, local, applied, |l| l.id.clone(), |a| a.id.clone());
            // Prisma names start with a timestamp; Rails versions are numbers of one width.
            let width = |m: &Migration| if tool == Tool::Rails { m.id.len() } else { 0 };
            all.sort_by(|a, b| (width(a), &a.id).cmp(&(width(b), &b.id)));
            all
        }
    }
}

/// A changeset is told apart by id, author and file name; the database records the file as
/// the changelog included it (`classpath:db/changelog/a.xml`), so only the last part counts.
fn liquibase_key(id: &str, author: Option<&str>, file: Option<&str>) -> String {
    let base = file
        .and_then(|f| f.rsplit(['/', '\\']).next())
        .unwrap_or("");
    format!("{id}\u{1}{}\u{1}{base}", author.unwrap_or(""))
}

fn migration(tool: Tool, id: &str, status: Status) -> Migration {
    Migration {
        tool,
        id: id.to_owned(),
        description: String::new(),
        file: None,
        status,
        applied_at: None,
        detail: None,
    }
}

fn differs(a: Option<&String>, b: Option<&String>) -> bool {
    matches!((a, b), (Some(x), Some(y)) if x != y)
}

/// The migration of a history row, with the file that matches it if there is one.
fn settled(tool: Tool, id: &str, row: &Applied, local: Option<&Local>) -> Migration {
    let (status, detail) = if row.state == AppliedState::Failed {
        (Status::Failed, row.note.clone())
    } else if row.state == AppliedState::Baseline {
        (Status::Applied, Some("baseline".to_owned()))
    } else if local.is_none() {
        (Status::Missing, Some("no file in the folder".to_owned()))
    } else if differs(
        row.checksum.as_ref(),
        local.and_then(|l| l.checksum.as_ref()),
    ) {
        let (db, file) = (row.checksum.clone(), local.and_then(|l| l.checksum.clone()));
        let detail = format!(
            "checksum differs (database {}, file {})",
            db.unwrap_or_default(),
            file.unwrap_or_default()
        );
        (Status::Divergent, Some(detail))
    } else {
        (Status::Applied, row.note.clone())
    };
    Migration {
        description: local
            .map(|l| l.description.clone())
            .or_else(|| row.description.clone())
            .unwrap_or_default(),
        file: local.map(|l| l.file.clone()),
        applied_at: row.at.clone(),
        detail,
        ..migration(tool, id, status)
    }
}

fn pending(tool: Tool, local: &Local, detail: Option<&str>) -> Migration {
    Migration {
        description: local.description.clone(),
        file: Some(local.file.clone()),
        detail: detail.map(str::to_owned),
        ..migration(tool, &local.id, Status::Pending)
    }
}

/// How well a row says the migration ran: when a migration has several rows (Prisma after a
/// rollback and a new run), the best one counts.
fn rank(state: AppliedState) -> u8 {
    match state {
        AppliedState::Done | AppliedState::Baseline => 0,
        AppliedState::Failed => 1,
        AppliedState::RolledBack => 2,
    }
}

/// The matching of tools that identify a migration by one key (Prisma, Rails, Liquibase).
fn keyed(
    tool: Tool,
    local: &[Local],
    applied: &[Applied],
    local_key: impl Fn(&Local) -> String,
    row_key: impl Fn(&Applied) -> String,
) -> Vec<Migration> {
    let mut order = Vec::new();
    let mut rows: HashMap<String, &Applied> = HashMap::new();
    for row in applied {
        let key = row_key(row);
        match rows.get(&key) {
            None => order.push(key.clone()),
            Some(old) if rank(old.state) < rank(row.state) => continue,
            Some(_) => {}
        }
        rows.insert(key, row);
    }
    let mut out = Vec::new();
    let mut matched = HashSet::new();
    for l in local {
        let key = local_key(l);
        match rows.get(&key) {
            Some(row) if row.state != AppliedState::RolledBack => {
                out.push(settled(tool, &l.id, row, Some(l)));
            }
            Some(_) => out.push(pending(tool, l, Some("rolled back"))),
            None => out.push(pending(tool, l, None)),
        }
        matched.insert(key);
    }
    for key in order.iter().filter(|k| !matched.contains(*k)) {
        let row = rows[key];
        if row.state != AppliedState::RolledBack {
            let mut m = settled(tool, &row.id, row, None);
            if let Some(author) = &row.author {
                m.description.clone_from(author);
            }
            out.push(m);
        }
    }
    out
}

/// Flyway versions compare part by part as numbers (`1.10` is after `1.9`), the shorter one
/// padded with zeros (`1` is `1.0`).
fn version_cmp(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<u128> {
        v.split(['.', '_'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    (0..a.len().max(b.len()))
        .map(|i| {
            let (x, y) = (a.get(i).unwrap_or(&0), b.get(i).unwrap_or(&0));
            x.cmp(y)
        })
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

fn flyway(local: &[Local], applied: &[Applied]) -> Vec<Migration> {
    let mut out = flyway_versioned(local, applied);
    out.sort_by(|a, b| version_cmp(&a.id, &b.id));
    out.extend(flyway_repeatable(local, applied));
    out
}

fn flyway_versioned(local: &[Local], applied: &[Applied]) -> Vec<Migration> {
    let rows: Vec<&Applied> = applied.iter().filter(|a| !a.id.is_empty()).collect();
    let highest = |state: AppliedState| {
        rows.iter()
            .filter(|a| a.state == state)
            .map(|a| a.id.as_str())
            .max_by(|a, b| version_cmp(a, b))
    };
    let (baseline, latest) = (highest(AppliedState::Baseline), highest(AppliedState::Done));
    let file_of = |v: &str| {
        local
            .iter()
            .find(|l| !l.id.is_empty() && version_cmp(&l.id, v).is_eq())
    };
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        // A version run again after a repair has several rows; the last one tells.
        if rows[i + 1..]
            .iter()
            .any(|r| version_cmp(&r.id, &row.id).is_eq())
        {
            continue;
        }
        out.push(settled(Tool::Flyway, &row.id, row, file_of(&row.id)));
    }
    for l in local.iter().filter(|l| !l.id.is_empty()) {
        if rows.iter().any(|r| version_cmp(&r.id, &l.id).is_eq()) {
            continue;
        }
        if baseline.is_some_and(|b| version_cmp(&l.id, b).is_le()) {
            let mut m = pending(Tool::Flyway, l, Some("below the baseline"));
            m.status = Status::Applied;
            out.push(m);
        } else if latest.is_some_and(|v| version_cmp(&l.id, v).is_lt()) {
            out.push(pending(Tool::Flyway, l, Some("out of order")));
        } else {
            out.push(pending(Tool::Flyway, l, None));
        }
    }
    out
}

/// Repeatable migrations have no version; Flyway runs one again when its checksum changes, so
/// a changed file is pending rather than divergent.
fn flyway_repeatable(local: &[Local], applied: &[Applied]) -> Vec<Migration> {
    let rows: Vec<&Applied> = applied.iter().filter(|a| a.id.is_empty()).collect();
    let last = |description: &str| {
        rows.iter()
            .rev()
            .find(|r| r.description.as_deref() == Some(description))
    };
    let mut out = Vec::new();
    for l in local.iter().filter(|l| l.id.is_empty()) {
        let mut m = match last(&l.description) {
            None => pending(Tool::Flyway, l, None),
            Some(row) if row.state == AppliedState::Failed => {
                settled(Tool::Flyway, "", row, Some(l))
            }
            Some(row) if differs(row.checksum.as_ref(), l.checksum.as_ref()) => {
                let mut m = pending(Tool::Flyway, l, Some("changed, will be applied again"));
                m.applied_at.clone_from(&row.at);
                m
            }
            Some(row) => settled(Tool::Flyway, "", row, Some(l)),
        };
        "R".clone_into(&mut m.id);
        out.push(m);
    }
    let mut seen = HashSet::new();
    for row in rows.iter().rev() {
        let description = row.description.clone().unwrap_or_default();
        let has_file = local
            .iter()
            .any(|l| l.id.is_empty() && l.description == description);
        if !has_file && seen.insert(description) {
            let mut m = settled(Tool::Flyway, "R", row, None);
            if row.state != AppliedState::Failed {
                m.status = Status::Missing;
            }
            out.push(m);
        }
    }
    out
}

fn alembic(local: &[Local], applied: &[Applied]) -> Vec<Migration> {
    let parents: HashMap<&str, &[String]> = local
        .iter()
        .map(|l| (l.id.as_str(), l.parents.as_slice()))
        .collect();
    let db_heads: Vec<&Applied> = applied
        .iter()
        .filter(|a| a.state == AppliedState::Done)
        .collect();
    // The heads and all that they follow ran.
    let mut done: HashSet<&str> = HashSet::new();
    let mut todo: Vec<&str> = db_heads.iter().map(|a| a.id.as_str()).collect();
    while let Some(id) = todo.pop() {
        if done.insert(id) {
            todo.extend(
                parents
                    .get(id)
                    .into_iter()
                    .flat_map(|p| p.iter().map(String::as_str)),
            );
        }
    }
    let followed: HashSet<&str> = local
        .iter()
        .flat_map(|l| l.parents.iter().map(String::as_str))
        .collect();
    let file_heads = local
        .iter()
        .filter(|l| !followed.contains(l.id.as_str()))
        .count();
    let mut depths = HashMap::new();
    let mut ordered: Vec<&Local> = local.iter().collect();
    ordered.sort_by_key(|l| (alembic_depth(&l.id, &parents, &mut depths, 0), l.id.clone()));
    let mut out = Vec::new();
    for l in ordered {
        let is_head = !followed.contains(l.id.as_str());
        let mut m = if done.contains(l.id.as_str()) {
            let row = db_heads.iter().find(|a| a.id == l.id).copied();
            let mut m = migration(Tool::Alembic, &l.id, Status::Applied);
            m.detail = row.map(|_| "head".to_owned());
            m
        } else {
            let detail = (is_head && file_heads > 1).then_some("branch head");
            pending(Tool::Alembic, l, detail)
        };
        m.description.clone_from(&l.description);
        m.file = Some(l.file.clone());
        out.push(m);
    }
    for row in db_heads
        .iter()
        .filter(|a| !parents.contains_key(a.id.as_str()))
    {
        out.push(settled(Tool::Alembic, &row.id, row, None));
    }
    out
}

/// How many revisions come before `id`; a cycle in broken files ends the count.
fn alembic_depth<'a>(
    id: &'a str,
    parents: &HashMap<&'a str, &'a [String]>,
    memo: &mut HashMap<&'a str, usize>,
    guard: usize,
) -> usize {
    if let Some(d) = memo.get(id) {
        return *d;
    }
    if guard > parents.len() {
        return 0;
    }
    let depth = parents.get(id).map_or(0, |ps| {
        ps.iter()
            .filter(|p| parents.contains_key(p.as_str()))
            .map(|p| 1 + alembic_depth(p, parents, memo, guard + 1))
            .max()
            .unwrap_or(0)
    });
    memo.insert(id, depth);
    depth
}

// ---------------------------------------------------------------------------------------------
// Reading the history tables
// ---------------------------------------------------------------------------------------------

/// For each tool found in `dir`, or whose history table exists in `schema`, the status of its
/// migrations. A missing history table is not an error: nothing was applied. The table is
/// looked for under its default name, whatever the case the database folds it to.
///
/// # Errors
///
/// When `dir` is not a folder, or the database cannot be read.
pub async fn status(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
    dir: &Path,
) -> Result<Vec<ToolStatus>> {
    if !dir.is_dir() {
        return Err(Error::Other(format!("{} is not a folder", dir.display())));
    }
    let mut found = detect(dir);
    let tables: Vec<String> = conn
        .objects(schema)
        .await?
        .into_iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(|o| o.name)
        .collect();
    let mut out = Vec::new();
    for tool in Tool::ALL {
        let local = found
            .iter_mut()
            .find(|f| f.tool == tool)
            .map(|f| std::mem::take(&mut f.files))
            .unwrap_or_default();
        let table = tables
            .iter()
            .find(|t| t.eq_ignore_ascii_case(tool.history_table()));
        let mut history_table = None;
        let mut rows = Vec::new();
        if let Some(table) = table
            && let Some(read) = read_history(conn, dialect, schema, tool, table).await?
        {
            history_table = Some(table.clone());
            rows = read;
        }
        if local.is_empty() && history_table.is_none() {
            continue;
        }
        let migrations = evaluate(tool, &local, &rows);
        out.push(ToolStatus {
            tool,
            history_table,
            migrations,
        });
    }
    Ok(out)
}

/// The columns to read from a history table: the name, and whether the table is not the
/// tool's without it.
type Columns = &'static [(&'static str, bool)];

/// Turns the rows read from a history table into [`Applied`].
type Convert = fn(&[TextRow]) -> Vec<Applied>;

/// The rows of a tool's history table as [`Applied`]; `None` when the table does not have the
/// tool's columns (another tool's table that happens to share the name).
async fn read_history(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
    tool: Tool,
    table: &str,
) -> Result<Option<Vec<Applied>>> {
    let (columns, convert): (Columns, Convert) = match tool {
        Tool::Flyway => (
            &[
                ("installed_rank", false),
                ("version", true),
                ("description", true),
                ("type", false),
                ("checksum", false),
                ("installed_on", false),
                ("success", true),
            ],
            flyway_rows,
        ),
        Tool::Liquibase => (
            &[
                ("ID", true),
                ("AUTHOR", true),
                ("FILENAME", true),
                ("DATEEXECUTED", false),
                ("EXECTYPE", false),
                ("ORDEREXECUTED", false),
            ],
            liquibase_rows,
        ),
        Tool::Prisma => (
            &[
                ("migration_name", true),
                ("checksum", false),
                ("finished_at", false),
                ("rolled_back_at", false),
            ],
            prisma_rows,
        ),
        Tool::Alembic => (&[("version_num", true)], single_id_rows),
        Tool::Rails => (&[("version", true)], single_id_rows),
    };
    Ok(select_columns(conn, dialect, schema, table, columns)
        .await?
        .map(|rows| convert(&rows)))
}

/// Reads `columns` of `schema.table`, found by name whatever their case. Each row has one value
/// per wanted column, `None` for an optional column the table lacks. `None` when a required
/// column is missing.
async fn select_columns(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
    table: &str,
    columns: Columns,
) -> Result<Option<Vec<TextRow>>> {
    let actual: Vec<String> = conn
        .table_details(schema, table)
        .await?
        .columns
        .into_iter()
        .map(|c| c.name)
        .collect();
    let mut present = Vec::new();
    let mut selected = Vec::new();
    for (name, required) in columns {
        match actual.iter().find(|c| c.eq_ignore_ascii_case(name)) {
            Some(c) => {
                present.push(true);
                selected.push(dialect.quote_ident(c).into_owned());
            }
            None if *required => return Ok(None),
            None => present.push(false),
        }
    }
    let target = dialect.qualified((!schema.is_empty()).then_some(schema), table);
    let sql = format!("SELECT {} FROM {target}", selected.join(", "));
    let rows = query_text(conn, &sql).await?;
    let spread = rows
        .into_iter()
        .map(|row| {
            let mut values = row.into_iter();
            present
                .iter()
                .map(|&p| if p { values.next().flatten() } else { None })
                .collect()
        })
        .collect();
    Ok(Some(spread))
}

fn flyway_rows(rows: &[TextRow]) -> Vec<Applied> {
    let mut rows: Vec<&TextRow> = rows.iter().collect();
    rows.sort_by_key(|r| {
        r[0].as_deref()
            .and_then(|n| n.parse::<i64>().ok())
            .unwrap_or(i64::MAX)
    });
    rows.into_iter()
        .filter_map(|r| {
            let kind = r.opt(3).unwrap_or_default().to_ascii_uppercase();
            if matches!(kind.as_str(), "SCHEMA" | "UNDO_SQL" | "DELETE") {
                return None;
            }
            let state = if !r.flag(6) && !r.opt(6).is_some_and(|s| s.eq_ignore_ascii_case("yes")) {
                AppliedState::Failed
            } else if kind == "BASELINE" {
                AppliedState::Baseline
            } else {
                AppliedState::Done
            };
            Some(Applied {
                description: Some(r.text(2)),
                checksum: r.opt(4),
                at: r.opt(5),
                ..Applied::new(r.text(1), state)
            })
        })
        .collect()
}

fn liquibase_rows(rows: &[TextRow]) -> Vec<Applied> {
    let mut rows: Vec<&TextRow> = rows.iter().collect();
    rows.sort_by_key(|r| r.number(5).unwrap_or(u64::MAX));
    rows.into_iter()
        .map(|r| {
            let exec = r.opt(4).unwrap_or_default().to_ascii_uppercase();
            let state = if exec == "FAILED" {
                AppliedState::Failed
            } else {
                AppliedState::Done
            };
            Applied {
                author: Some(r.text(1)),
                file: Some(r.text(2)),
                at: r.opt(3),
                note: (!exec.is_empty() && exec != "EXECUTED").then(|| exec.to_ascii_lowercase()),
                ..Applied::new(r.text(0), state)
            }
        })
        .collect()
}

fn prisma_rows(rows: &[TextRow]) -> Vec<Applied> {
    rows.iter()
        .map(|r| {
            let state = if r.opt(3).is_some() {
                AppliedState::RolledBack
            } else if r.opt(2).is_none() {
                AppliedState::Failed
            } else {
                AppliedState::Done
            };
            Applied {
                checksum: r.opt(1),
                at: r.opt(2),
                ..Applied::new(r.text(0), state)
            }
        })
        .collect()
}

fn single_id_rows(rows: &[TextRow]) -> Vec<Applied> {
    rows.iter()
        .map(|r| Applied::new(r.text(0), AppliedState::Done))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn files_of(found: &[Found], tool: Tool) -> Vec<Local> {
        found
            .iter()
            .find(|f| f.tool == tool)
            .map(|f| f.files.clone())
            .unwrap_or_default()
    }

    fn row(id: &str, state: AppliedState) -> Applied {
        Applied::new(id.to_owned(), state)
    }

    fn statuses(m: &[Migration]) -> Vec<(&str, Status)> {
        m.iter().map(|m| (m.id.as_str(), m.status)).collect()
    }

    #[test]
    fn flyway_checksum_reads_lines_without_their_terminators() {
        // Flyway feeds each line to a CRC32 without its terminator, so CRLF, LF and CR files
        // that hold "1234", "5678", "9" all give the CRC32 of "123456789", whose standard
        // check value is 0xCBF43926. As a signed 32-bit number that is 3421780262 - 2^32.
        let expected = -873_187_034;
        assert_eq!(flyway_checksum(b"1234\r\n5678\n9"), expected);
        assert_eq!(flyway_checksum(b"1234\r5678\r\n9\n"), expected);
        // The byte order mark of the first line is not part of the checksum.
        assert_eq!(flyway_checksum(b"\xEF\xBB\xBF123456789"), expected);
        assert_eq!(flyway_checksum(b""), 0);
    }

    #[test]
    fn prisma_checksum_is_the_sha256_in_hex() {
        assert_eq!(
            prisma_checksum(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn flyway_names_are_told_from_other_sql_files() {
        let v = flyway_name("V1_2__add_users.sql").unwrap();
        assert_eq!(v, ("1.2".to_owned(), "add users".to_owned(), false));
        assert_eq!(
            flyway_name("R__refresh_views.sql").unwrap(),
            (String::new(), "refresh views".to_owned(), true)
        );
        assert!(flyway_name("V1.sql").is_none());
        assert!(flyway_name("Vx__a.sql").is_none());
        assert!(flyway_name("schema.sql").is_none());
        assert_eq!(version_cmp("1.10", "1.9"), Ordering::Greater);
        assert_eq!(version_cmp("1", "1.0"), Ordering::Equal);
        assert_eq!(version_cmp("2", "10"), Ordering::Less);
    }

    #[test]
    fn detects_a_flyway_project_and_skips_dependency_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "src/main/resources/db/migration/V1__init.sql",
            "CREATE TABLE a (id int);",
        );
        write(root, "src/main/resources/db/migration/V2__more.sql", "x");
        write(root, "sql/R__views.sql", "CREATE VIEW v AS SELECT 1;");
        write(root, "node_modules/pkg/V9__no.sql", "x");
        write(root, "target/V8__no.sql", "x");
        write(root, "a/b/c/d/e/f/g/V7__too_deep.sql", "x");
        let found = detect(root);
        assert_eq!(found.len(), 1);
        let files = files_of(&found, Tool::Flyway);
        let ids: Vec<_> = files.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["", "1", "2"]);
        assert_eq!(
            files[1].file,
            "src/main/resources/db/migration/V1__init.sql"
        );
        assert_eq!(
            files[1].checksum.as_deref(),
            Some(
                flyway_checksum(b"CREATE TABLE a (id int);")
                    .to_string()
                    .as_str()
            )
        );
    }

    #[test]
    fn flyway_statuses() {
        let local = |v: &str, sum: &str| Local {
            id: v.to_owned(),
            description: format!("m{v}"),
            file: format!("V{v}__m.sql"),
            checksum: Some(sum.to_owned()),
            ..Local::default()
        };
        let files = [
            local("1", "10"),
            local("2", "20"),
            local("3", "30"),
            local("4", "40"),
            local("5", "50"),
        ];
        let done = |v: &str, sum: &str, state| Applied {
            checksum: Some(sum.to_owned()),
            at: Some("2026-01-01 10:00:00".to_owned()),
            ..Applied::new(v.to_owned(), state)
        };
        let rows = [
            done("1", "10", AppliedState::Done),
            done("2", "99", AppliedState::Done),
            done("4", "40", AppliedState::Failed),
            done("6", "60", AppliedState::Done),
            done("7", "70", AppliedState::Done),
        ];
        // 3 is older than the latest applied (7): pending and out of order; 5 too.
        let m = evaluate(Tool::Flyway, &files, &rows);
        assert_eq!(
            statuses(&m),
            [
                ("1", Status::Applied),
                ("2", Status::Divergent),
                ("3", Status::Pending),
                ("4", Status::Failed),
                ("5", Status::Pending),
                ("6", Status::Missing),
                ("7", Status::Missing),
            ]
        );
        assert_eq!(m[2].detail.as_deref(), Some("out of order"));
        assert_eq!(
            m[1].detail.as_deref(),
            Some("checksum differs (database 99, file 20)")
        );
        assert_eq!(m[0].applied_at.as_deref(), Some("2026-01-01 10:00:00"));
        assert!(m[5].file.is_none());
        // Nothing applied: everything is pending, nothing out of order.
        let fresh = evaluate(Tool::Flyway, &files, &[]);
        assert!(
            fresh
                .iter()
                .all(|m| m.status == Status::Pending && m.detail.is_none())
        );
    }

    #[test]
    fn flyway_baseline_and_repeatable_migrations() {
        let v = |id: &str| Local {
            id: id.to_owned(),
            file: format!("V{id}__x.sql"),
            ..Local::default()
        };
        let repeatable = |sum: &str| Local {
            description: "views".to_owned(),
            file: "R__views.sql".to_owned(),
            checksum: Some(sum.to_owned()),
            ..Local::default()
        };
        let baseline = row("2", AppliedState::Baseline);
        let m = evaluate(Tool::Flyway, &[v("1"), v("2"), v("3")], &[baseline]);
        assert_eq!(
            statuses(&m),
            [
                ("1", Status::Applied),
                ("2", Status::Applied),
                ("3", Status::Pending)
            ]
        );
        assert_eq!(m[0].detail.as_deref(), Some("below the baseline"));
        let applied = Applied {
            description: Some("views".to_owned()),
            checksum: Some("1".to_owned()),
            ..Applied::new(String::new(), AppliedState::Done)
        };
        let same = evaluate(
            Tool::Flyway,
            &[repeatable("1")],
            std::slice::from_ref(&applied),
        );
        assert_eq!(statuses(&same), [("R", Status::Applied)]);
        let edited = evaluate(
            Tool::Flyway,
            &[repeatable("2")],
            std::slice::from_ref(&applied),
        );
        assert_eq!(statuses(&edited), [("R", Status::Pending)]);
        assert_eq!(
            edited[0].detail.as_deref(),
            Some("changed, will be applied again")
        );
        let gone = evaluate(Tool::Flyway, &[], &[applied]);
        assert_eq!(statuses(&gone), [("R", Status::Missing)]);
    }

    #[test]
    fn detects_liquibase_changesets_in_xml_yaml_and_sql() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "db/changelog/a.xml",
            r#"<?xml version="1.0"?>
<databaseChangeLog xmlns="http://www.liquibase.org/xml/ns/dbchangelog">
  <changeSet id="1" author="ann"><createTable tableName="t"/></changeSet>
  <changeSet author="bob" id="2"/>
</databaseChangeLog>"#,
        );
        write(
            root,
            "pom.xml",
            "<project><artifactId>x</artifactId></project>",
        );
        write(
            root,
            "db/changelog/b.yaml",
            "databaseChangeLog:\n  - changeSet:\n      id: 3\n      author: 'cy'\n      changes:\n        - sql:\n            sql: select 1\n  - changeSet:\n      author: di\n      id: \"4\"\n",
        );
        write(
            root,
            "db/changelog/c.sql",
            "--liquibase formatted sql\n\n--changeset eve:5 runOnChange:true\nselect 1;\n-- changeset fay:6\nselect 2;\n",
        );
        write(root, "db/other.sql", "-- changeset x:y\nselect 1;");
        let found = detect(root);
        let sets: Vec<(String, String, String)> = files_of(&found, Tool::Liquibase)
            .into_iter()
            .map(|l| (l.id, l.author.unwrap(), l.file))
            .collect();
        let sets: Vec<(&str, &str, &str)> = sets
            .iter()
            .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
            .collect();
        assert_eq!(
            sets,
            [
                ("1", "ann", "db/changelog/a.xml"),
                ("2", "bob", "db/changelog/a.xml"),
                ("3", "cy", "db/changelog/b.yaml"),
                ("4", "di", "db/changelog/b.yaml"),
                ("5", "eve", "db/changelog/c.sql"),
                ("6", "fay", "db/changelog/c.sql"),
            ]
        );
    }

    #[test]
    fn liquibase_statuses_match_by_id_author_and_file_name() {
        let set = |id: &str, author: &str| Local {
            id: id.to_owned(),
            description: author.to_owned(),
            file: "db/changelog/a.xml".to_owned(),
            author: Some(author.to_owned()),
            ..Local::default()
        };
        let db = |id: &str, author: &str, state| Applied {
            author: Some(author.to_owned()),
            file: Some("classpath:db/changelog/a.xml".to_owned()),
            ..Applied::new(id.to_owned(), state)
        };
        let m = evaluate(
            Tool::Liquibase,
            &[set("1", "ann"), set("2", "ann"), set("3", "ann")],
            &[
                db("1", "ann", AppliedState::Done),
                db("2", "ann", AppliedState::Failed),
                db("9", "zed", AppliedState::Done),
            ],
        );
        assert_eq!(
            statuses(&m),
            [
                ("1", Status::Applied),
                ("2", Status::Failed),
                ("3", Status::Pending),
                ("9", Status::Missing),
            ]
        );
        assert_eq!(m[3].description, "zed");
    }

    #[test]
    fn prisma_migrations_and_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "prisma/migrations/20260101_init/migration.sql",
            "CREATE TABLE a (id int);",
        );
        write(
            root,
            "prisma/migrations/20260102_users/migration.sql",
            "CREATE TABLE u (id int);",
        );
        write(root, "prisma/migrations/20260103_x/migration.sql", "x");
        write(root, "prisma/migrations/20260104_y/migration.sql", "y");
        write(
            root,
            "prisma/migrations/migration_lock.toml",
            "provider = \"sqlite\"",
        );
        write(root, "other/migrations/z/migration.sql", "not prisma");
        let files = files_of(&detect(root), Tool::Prisma);
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].id, "20260101_init");
        let sum = |i: usize| files[i].checksum.clone();
        let db = |name: &str, checksum: Option<String>, state| Applied {
            checksum,
            at: Some("t".to_owned()),
            ..Applied::new(name.to_owned(), state)
        };
        let rows = [
            db("20260101_init", sum(0), AppliedState::Done),
            db("20260102_users", Some("00".to_owned()), AppliedState::Done),
            db("20260103_x", sum(2), AppliedState::Failed),
            db("20260104_y", sum(3), AppliedState::RolledBack),
            db("20259999_gone", None, AppliedState::Done),
            db("20259998_rolled", None, AppliedState::RolledBack),
        ];
        let m = evaluate(Tool::Prisma, &files, &rows);
        assert_eq!(
            statuses(&m),
            [
                ("20259999_gone", Status::Missing),
                ("20260101_init", Status::Applied),
                ("20260102_users", Status::Divergent),
                ("20260103_x", Status::Failed),
                ("20260104_y", Status::Pending),
            ]
        );
        assert_eq!(m[4].detail.as_deref(), Some("rolled back"));
    }

    #[test]
    fn alembic_revisions_and_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = |rev: &str, down: &str, title: &str| {
            format!(
                "\"\"\"{title}\n\nRevision ID: {rev}\n\"\"\"\nrevision = '{rev}'\ndown_revision = {down}\n"
            )
        };
        write(
            root,
            "alembic/versions/a1_init.py",
            &file("a1", "None", "create users"),
        );
        write(
            root,
            "alembic/versions/b2_next.py",
            &file("b2", "'a1'", "add email"),
        );
        write(
            root,
            "alembic/versions/c3_left.py",
            &file("c3", "'b2'", "left branch"),
        );
        write(
            root,
            "alembic/versions/d4_right.py",
            &file("d4", "'b2'", "right branch"),
        );
        write(
            root,
            "alembic/versions/e5_merge.py",
            "revision: str = \"e5\"\ndown_revision: Union[str, None] = (\n    \"c3\",\n    \"d4\",\n)\n",
        );
        write(root, "src/versions/notes.py", "x = 1");
        let files = files_of(&detect(root), Tool::Alembic);
        assert_eq!(files.len(), 5);
        let e5 = files.iter().find(|l| l.id == "e5").unwrap();
        assert_eq!(e5.parents, ["c3", "d4"]);
        assert_eq!(e5.description, "merge");
        let a1 = files.iter().find(|l| l.id == "a1").unwrap();
        assert_eq!(a1.parents.len(), 0);
        assert_eq!(a1.description, "create users");

        // The database is at the left branch: its ancestors ran, the right branch did not.
        let m = evaluate(Tool::Alembic, &files, &[row("c3", AppliedState::Done)]);
        assert_eq!(
            statuses(&m),
            [
                ("a1", Status::Applied),
                ("b2", Status::Applied),
                ("c3", Status::Applied),
                ("d4", Status::Pending),
                ("e5", Status::Pending),
            ]
        );
        assert_eq!(m[2].detail.as_deref(), Some("head"));
        // Two heads in the DB after a branch: both lines ran, only the merge is left.
        let both = evaluate(
            Tool::Alembic,
            &files,
            &[row("c3", AppliedState::Done), row("d4", AppliedState::Done)],
        );
        assert_eq!(both[3].status, Status::Applied);
        assert_eq!(both[4].status, Status::Pending);
        // A head the files do not know is missing; nothing applied is all pending.
        let unknown = evaluate(Tool::Alembic, &files, &[row("zz", AppliedState::Done)]);
        assert_eq!(unknown.last().map(|m| m.status), Some(Status::Missing));
        assert!(unknown[..5].iter().all(|m| m.status == Status::Pending));
        let branches = evaluate(Tool::Alembic, &files[..4], &[]);
        assert_eq!(branches[2].detail.as_deref(), Some("branch head"));
    }

    #[test]
    fn rails_migrations_and_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "db/migrate/20260101000000_create_users.rb",
            "class A; end",
        );
        write(
            root,
            "db/migrate/20260102000000_add_email.rb",
            "class B; end",
        );
        write(root, "db/migrate/README.md", "x");
        write(root, "app/migrate/20260103000000_no.rb", "x");
        let files = files_of(&detect(root), Tool::Rails);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].description, "create users");
        let m = evaluate(
            Tool::Rails,
            &files,
            &[
                row("20260101000000", AppliedState::Done),
                row("20250101000000", AppliedState::Done),
            ],
        );
        assert_eq!(
            statuses(&m),
            [
                ("20250101000000", Status::Missing),
                ("20260101000000", Status::Applied),
                ("20260102000000", Status::Pending),
            ]
        );
    }

    #[test]
    fn history_rows_are_read_from_text() {
        let text = |v: &[&str]| -> TextRow {
            v.iter()
                .map(|s| (!s.is_empty()).then(|| (*s).to_owned()))
                .collect()
        };
        // rank, version, description, type, checksum, installed_on, success
        let rows = flyway_rows(&[
            text(&["3", "2", "b", "SQL", "5", "t", "0"]),
            text(&[
                "1",
                "",
                "<< Flyway Create Schema >>",
                "SCHEMA",
                "",
                "t",
                "1",
            ]),
            text(&["2", "1", "a", "SQL", "-7", "t", "true"]),
        ]);
        let ids: Vec<_> = rows.iter().map(|r| (r.id.as_str(), r.state)).collect();
        assert_eq!(
            ids,
            [("1", AppliedState::Done), ("2", AppliedState::Failed)]
        );
        assert_eq!(rows[0].checksum.as_deref(), Some("-7"));
        let prisma = prisma_rows(&[
            text(&["a", "c", "t", ""]),
            text(&["b", "c", "", ""]),
            text(&["c", "c", "t", "t"]),
        ]);
        let states: Vec<_> = prisma.iter().map(|r| r.state).collect();
        assert_eq!(
            states,
            [
                AppliedState::Done,
                AppliedState::Failed,
                AppliedState::RolledBack
            ]
        );
    }

    #[tokio::test]
    async fn status_reads_the_history_tables_of_a_sqlite_database() {
        use platypus_core::{ConnectionConfig, DriverKind, ExecOptions};
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "db/V1__a.sql", "CREATE TABLE a (id int);");
        write(dir.path(), "db/V2__b.sql", "CREATE TABLE b (id int);");
        write(dir.path(), "db/migrate/20260101000000_x.rb", "");
        let mut config = ConnectionConfig::new(DriverKind::Sqlite);
        config.database = dir.path().join("app.db").to_string_lossy().into_owned();
        let conn = platypus_driver_sqlite::SqliteConnection::connect(&config)
            .await
            .unwrap();
        let sum = flyway_checksum(b"CREATE TABLE a (id int);");
        let seed = format!(
            "CREATE TABLE FLYWAY_SCHEMA_HISTORY (installed_rank INTEGER, version TEXT, description TEXT, type TEXT, \
               script TEXT, checksum INTEGER, installed_on TEXT, success INTEGER);
             INSERT INTO FLYWAY_SCHEMA_HISTORY VALUES (1, '1', 'a', 'SQL', 'V1__a.sql', {sum}, 't', 1);
             CREATE TABLE schema_migrations (version TEXT)"
        );
        for sql in seed.split(';') {
            conn.execute(sql, &ExecOptions::default()).await.unwrap();
        }
        let found = status(&conn, Dialect::Sqlite, "main", dir.path())
            .await
            .unwrap();
        let tools: Vec<_> = found.iter().map(|f| f.tool).collect();
        assert_eq!(tools, [Tool::Flyway, Tool::Rails]);
        assert_eq!(
            found[0].history_table.as_deref(),
            Some("FLYWAY_SCHEMA_HISTORY")
        );
        assert_eq!(
            statuses(&found[0].migrations),
            [("1", Status::Applied), ("2", Status::Pending)]
        );
        // An empty history table: the file is pending.
        assert_eq!(
            statuses(&found[1].migrations),
            [("20260101000000", Status::Pending)]
        );
        assert!(
            status(&conn, Dialect::Sqlite, "main", &dir.path().join("nope"))
                .await
                .is_err()
        );
    }

    #[test]
    fn python_values_are_read_without_running_python() {
        assert_eq!(quoted("None"), Vec::<String>::new());
        assert_eq!(quoted("'a1'  # comment 'x'"), ["a1"]);
        assert_eq!(quoted("(\"a\", 'b')"), ["a", "b"]);
    }
}
