//! What masks values for agents, sealed with the rest of what the user approved (see
//! [`crate::policy`]).
//!
//! The rules on names and types live in `settings.toml` and the marks on columns in `catalog.db`:
//! files an agent with file tools can edit to read the values. So the seal holds them too: the
//! rules in `agent-policy.json`, and the marks, which can be many, in `agent-policy-marks.json`,
//! whose SHA-256 is in the policy. The check of the seal reads the policy as before; a process
//! reads the marks once, and again only when the sealed hash changes.
//!
//! Agents get the union of what is sealed and what the files say: an addition applies at once, and
//! a removal waits until the user accepts it in the app. A mark "not sensitive" lifts the rules, so
//! it counts only when both say so. When the seal no longer matches, what was approved cannot be
//! told (its rules and marks may be what was removed), so every value reads masked until the user
//! answers. Without a seal the files are taken as they are, as for the rest of the policy.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use platypus_sql::sensitive::DEFAULT_NAMES;
use platypus_store::{CatalogKey, SavedConnection, SensitiveColumn, SettingsStore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::policy::{AgentPolicy, Loosening, Seal, Sealed};

/// The file of the approved marks, beside the policy.
pub const MARKS_FILE: &str = "agent-policy-marks.json";
/// The setting with the names of sensitive columns.
pub const MASK_NAMES: &str = "safety.mask_names";
/// The setting with the types of sensitive columns.
pub const MASK_TYPES: &str = "safety.mask_types";

/// The rule that masks every column.
const EVERYTHING: &str = "*";
/// Columns a question names, of the marks it is about.
const NAMED_COLUMNS: usize = 5;

/// The patterns of a rule: comma-separated, as `NameRules` reads them.
fn patterns(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|p| !p.is_empty())
}

/// The patterns of `list`, as `NameRules` compares them.
fn pattern_set(list: &str) -> HashSet<String> {
    patterns(list).map(str::to_lowercase).collect()
}

/// `first`, then the patterns of `then` it does not have.
fn union_of(first: &str, then: &str) -> String {
    if first == then {
        return first.to_owned();
    }
    let have = pattern_set(first);
    let mut all: Vec<&str> = patterns(first).collect();
    let before = all.len();
    all.extend(patterns(then).filter(|p| !have.contains(&p.to_lowercase())));
    if all.len() == before {
        first.to_owned()
    } else {
        all.join(", ")
    }
}

/// The patterns of `approved` that `files` no longer has.
fn removed_patterns(files: &str, approved: &str) -> Vec<String> {
    let have = pattern_set(files);
    patterns(approved)
        .filter(|p| !have.contains(&p.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

/// `approved` after the user changed the file's `before` to `after` in the app: what they took out
/// goes and what they added comes. A pattern missing from `before` was not in what they saw, so it
/// stays until they answer about it.
fn edited(approved: &str, before: &str, after: &str) -> String {
    let (before, now) = (pattern_set(before), pattern_set(after));
    let kept: Vec<&str> = patterns(approved)
        .filter(|p| {
            let p = p.to_lowercase();
            now.contains(&p) || !before.contains(&p)
        })
        .collect();
    union_of(&kept.join(", "), after)
}

/// Which rule a change is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskedBy {
    Name,
    Type,
}

/// The rules of Settings › Safety that tell the sensitive columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskingRules {
    pub names: String,
    pub types: String,
}

impl Default for MaskingRules {
    /// The app's defaults.
    fn default() -> Self {
        Self {
            names: DEFAULT_NAMES.to_owned(),
            types: String::new(),
        }
    }
}

impl MaskingRules {
    /// The rules `settings` say; the defaults for what they leave out.
    #[must_use]
    pub fn of(settings: Option<&SettingsStore>) -> Self {
        let rule = |key: &str| settings.and_then(|s| s.get::<String>(key));
        Self {
            names: rule(MASK_NAMES).unwrap_or_else(|| DEFAULT_NAMES.to_owned()),
            types: rule(MASK_TYPES).unwrap_or_default(),
        }
    }

    /// What masks when either `self` or `other` does.
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            names: union_of(&self.names, &other.names),
            types: union_of(&self.types, &other.types),
        }
    }

    /// The patterns of `approved` these rules do not have, for the question to the user.
    #[must_use]
    pub fn removed_from(&self, approved: &Self) -> Vec<Loosening> {
        [
            (MaskedBy::Name, &self.names, &approved.names),
            (MaskedBy::Type, &self.types, &approved.types),
        ]
        .into_iter()
        .filter_map(|(by, files, approved)| {
            let patterns = removed_patterns(files, approved);
            (!patterns.is_empty()).then_some(Loosening::MaskingRules { by, patterns })
        })
        .collect()
    }

    /// These rules, as approved, after the user changed the setting `key` from `before` to `after`
    /// in the app; `None` is the setting's default.
    pub fn apply_edit(&mut self, key: &str, before: Option<&str>, after: Option<&str>) {
        let (rule, default) = match key {
            MASK_NAMES => (&mut self.names, DEFAULT_NAMES),
            MASK_TYPES => (&mut self.types, ""),
            _ => return,
        };
        *rule = edited(rule, before.unwrap_or(default), after.unwrap_or(default));
    }
}

/// What a column is when the approved mark is `approved` and the file's is `file`: sensitive when
/// either says so, "not sensitive" only when both do, and left to the rules otherwise.
fn honored_mark(approved: Option<bool>, file: Option<bool>) -> Option<bool> {
    match (approved, file) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// The marks of one connection.
type Marks = HashMap<CatalogKey, bool>;

/// `file`, the marks of a connection in the catalog, as agents get them with `approved` sealed.
fn honored_marks(approved: Option<&Marks>, mut file: Vec<SensitiveColumn>) -> Vec<SensitiveColumn> {
    let mut found = 0;
    file.retain_mut(|mark| {
        let sealed = approved.and_then(|a| a.get(&mark.key)).copied();
        found += usize::from(sealed == Some(true));
        let honored = honored_mark(sealed, Some(mark.sensitive));
        mark.sensitive = honored == Some(true);
        honored.is_some()
    });
    let Some(approved) = approved else {
        return file;
    };
    // Nearly always all of them: then the catalog holds nothing to add.
    if found < approved.values().filter(|sensitive| **sensitive).count() {
        let have: HashSet<CatalogKey> = file.iter().map(|m| m.key.clone()).collect();
        file.extend(
            approved
                .iter()
                .filter(|(key, sensitive)| **sensitive && !have.contains(*key))
                .map(|(key, _)| SensitiveColumn {
                    key: key.clone(),
                    sensitive: true,
                }),
        );
    }
    file
}

/// The marks of the columns of one table in the marks file: a column's table written once.
#[derive(Debug, Serialize, Deserialize)]
struct TableMarks {
    #[serde(rename = "d", default, skip_serializing_if = "String::is_empty")]
    database: String,
    #[serde(rename = "s")]
    schema: String,
    #[serde(rename = "o")]
    object: String,
    #[serde(rename = "y", default, skip_serializing_if = "Vec::is_empty")]
    sensitive: Vec<String>,
    #[serde(rename = "n", default, skip_serializing_if = "Vec::is_empty")]
    not_sensitive: Vec<String>,
}

/// The columns of each table (database, schema, object), sensitive and not sensitive.
type ColumnsByTable<'a> = BTreeMap<(&'a str, &'a str, &'a str), (Vec<&'a str>, Vec<&'a str>)>;

/// The marks on columns, by connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SealedMarks(HashMap<Uuid, Marks>);

impl SealedMarks {
    /// The marks each connection has, one that has none included: what the catalog holds of the
    /// saved connections.
    #[must_use]
    pub fn from_catalog(marks: impl IntoIterator<Item = (Uuid, Vec<SensitiveColumn>)>) -> Self {
        Self(
            marks
                .into_iter()
                .map(|(id, marks)| {
                    let marks = marks.into_iter().map(|m| (m.key, m.sensitive)).collect();
                    (id, marks)
                })
                .collect(),
        )
    }

    /// How many marks there are.
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.values().map(HashMap::len).sum()
    }

    /// The user marked `key` in the app; `None` leaves it to the rules again.
    pub fn set(&mut self, connection: Uuid, key: &CatalogKey, sensitive: Option<bool>) {
        let marks = self.0.entry(connection).or_default();
        match sensitive {
            Some(sensitive) => {
                marks.insert(key.clone(), sensitive);
            }
            None => {
                marks.remove(key);
            }
        }
    }

    /// The user deleted `connection` in the app.
    pub fn forget(&mut self, connection: Uuid) {
        self.0.remove(&connection);
    }

    /// The app renamed an object or a column: as `CatalogStore::rename`, the marks follow it.
    pub fn rename(&mut self, connection: Uuid, from: &CatalogKey, to: &CatalogKey) {
        let Some(marks) = self.0.get_mut(&connection) else {
            return;
        };
        let follows = |key: &CatalogKey| {
            key.database == from.database
                && key.schema == from.schema
                && key.object == from.object
                && (from.column.is_empty() || key.column == from.column)
        };
        let moving: Vec<CatalogKey> = marks.keys().filter(|key| follows(key)).cloned().collect();
        for key in moving {
            let Some(sensitive) = marks.remove(&key) else {
                continue;
            };
            let moved = if from.column.is_empty() {
                CatalogKey {
                    schema: to.schema.clone(),
                    object: to.object.clone(),
                    ..key
                }
            } else {
                CatalogKey {
                    column: to.column.clone(),
                    ..key
                }
            };
            marks.insert(moved, sensitive);
        }
    }

    /// These marks, as approved, with the additions of `file`: what stays sealed.
    #[must_use]
    pub fn within(&self, file: &Self) -> Self {
        let ids: HashSet<&Uuid> = self.0.keys().chain(file.0.keys()).collect();
        let empty = Marks::new();
        Self(
            ids.into_iter()
                .filter_map(|id| {
                    let (approved, file) = (
                        self.0.get(id).unwrap_or(&empty),
                        file.0.get(id).unwrap_or(&empty),
                    );
                    let marks: Marks = approved
                        .keys()
                        .chain(file.keys())
                        .filter_map(|key| {
                            honored_mark(approved.get(key).copied(), file.get(key).copied())
                                .map(|mark| (key.clone(), mark))
                        })
                        .collect();
                    (!marks.is_empty()).then_some((*id, marks))
                })
                .collect(),
        )
    }

    /// The columns of connection `id` that `file` leaves less masked than these marks, as
    /// approved, and what puts each back.
    fn loosened(&self, id: Uuid, file: &Marks) -> Vec<(CatalogKey, Option<bool>)> {
        let empty = Marks::new();
        let approved = self.0.get(&id).unwrap_or(&empty);
        approved
            .iter()
            .filter(|(key, _)| !file.contains_key(*key))
            .map(|(key, _)| key)
            .chain(file.keys())
            .filter_map(|key| {
                let (sealed, in_file) = (approved.get(key).copied(), file.get(key).copied());
                let honored = honored_mark(sealed, in_file);
                (honored != in_file).then(|| (key.clone(), honored))
            })
            .collect()
    }

    /// What `file` gives agents beyond these marks, as approved, on the saved `connections`.
    #[must_use]
    pub fn lifted_by(&self, file: &Self, connections: &[SavedConnection]) -> Vec<Loosening> {
        connections
            .iter()
            .filter_map(|c| {
                let mut columns: Vec<String> = self
                    .loosened(c.id, file.0.get(&c.id)?)
                    .into_iter()
                    .map(|(key, _)| format!("{}.{}.{}", key.schema, key.object, key.column))
                    .collect();
                let count = columns.len();
                columns.sort_unstable();
                columns.truncate(NAMED_COLUMNS);
                (count > 0).then(|| Loosening::SensitiveMarks {
                    id: c.id,
                    name: c.name.clone(),
                    count,
                    columns,
                })
            })
            .collect()
    }

    /// What to write to the catalog so that `file` holds no more than these marks, as approved:
    /// the mark of each column, `None` to take it away.
    #[must_use]
    pub fn restoring(&self, file: &Self) -> Vec<(Uuid, CatalogKey, Option<bool>)> {
        file.0
            .iter()
            .flat_map(|(id, marks)| {
                self.loosened(*id, marks)
                    .into_iter()
                    .map(|(key, mark)| (*id, key, mark))
            })
            .collect()
    }

    /// The marks file: sorted, so that the same marks give the same bytes.
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut all: BTreeMap<Uuid, ColumnsByTable<'_>> = BTreeMap::new();
        for (id, marks) in &self.0 {
            if marks.is_empty() {
                continue;
            }
            let tables = all.entry(*id).or_default();
            for (key, sensitive) in marks {
                let table = (&*key.database, &*key.schema, &*key.object);
                let (yes, no) = tables.entry(table).or_default();
                if *sensitive { yes } else { no }.push(&key.column);
            }
        }
        let file: BTreeMap<Uuid, Vec<TableMarks>> = all
            .into_iter()
            .map(|(id, tables)| {
                let tables = tables
                    .into_iter()
                    .map(|((database, schema, object), (mut yes, mut no))| {
                        yes.sort_unstable();
                        no.sort_unstable();
                        TableMarks {
                            database: database.to_owned(),
                            schema: schema.to_owned(),
                            object: object.to_owned(),
                            sensitive: yes.into_iter().map(str::to_owned).collect(),
                            not_sensitive: no.into_iter().map(str::to_owned).collect(),
                        }
                    })
                    .collect();
                (id, tables)
            })
            .collect();
        serde_json::to_vec(&file).map_err(|e| e.to_string())
    }

    /// The marks of a marks file; `None` when it is not one.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let file: BTreeMap<Uuid, Vec<TableMarks>> = serde_json::from_slice(bytes).ok()?;
        let marks = file.into_iter().map(|(id, tables)| {
            let mut marks = Marks::new();
            for table in tables {
                let key = |column: String| CatalogKey {
                    database: table.database.clone(),
                    schema: table.schema.clone(),
                    object: table.object.clone(),
                    column,
                };
                marks.extend(table.sensitive.iter().map(|c| (key(c.clone()), true)));
                marks.extend(table.not_sensitive.iter().map(|c| (key(c.clone()), false)));
            }
            (id, marks)
        });
        Some(Self(marks.collect()))
    }
}

/// What the seal says about masking, for what agents read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskingSeal {
    /// No seal, or the policy of a version that sealed no masking: the files as they are.
    Files,
    /// What the user approved.
    Approved {
        rules: MaskingRules,
        marks: Arc<SealedMarks>,
    },
    /// The seal no longer matches: what was approved cannot be told, so every value masks.
    Lost,
}

impl MaskingSeal {
    /// The rules agents get when the files say `files`.
    #[must_use]
    pub fn rules(&self, files: MaskingRules) -> MaskingRules {
        match self {
            Self::Files => files,
            Self::Approved { rules, .. } if files == *rules => files,
            Self::Approved { rules, .. } => files.union(rules),
            Self::Lost => MaskingRules {
                names: EVERYTHING.to_owned(),
                types: EVERYTHING.to_owned(),
            },
        }
    }

    /// The marks agents get on `connection` when the catalog says `file`.
    #[must_use]
    pub fn marks(&self, connection: Uuid, file: Vec<SensitiveColumn>) -> Vec<SensitiveColumn> {
        match self {
            Self::Files => file,
            Self::Approved { marks, .. } => honored_marks(marks.0.get(&connection), file),
            Self::Lost => honored_marks(None, file),
        }
    }
}

/// What the user's "undo" leaves: the policy and marks to seal, and what to put back in the
/// catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undo {
    pub policy: AgentPolicy,
    pub marks: SealedMarks,
    pub restore: Vec<(Uuid, CatalogKey, Option<bool>)>,
}

/// What the question says when the seal no longer matches: nothing can be compared with what was
/// approved, so the changes are those from the app's defaults, a mark "not sensitive" included.
fn lost_seal(
    current: &AgentPolicy,
    connections: &[SavedConnection],
    check_marks: bool,
    file_marks: &dyn Fn() -> Result<SealedMarks, String>,
) -> Vec<Loosening> {
    let mut found = Sealed::Broken.loosenings(current, connections);
    if check_marks && let Ok(file) = file_marks() {
        found.extend(SealedMarks::default().lifted_by(&file, connections));
    }
    found
}

impl Seal {
    /// What the seal `sealed` says about masking.
    #[must_use]
    pub fn masking(&self, sealed: &Sealed) -> MaskingSeal {
        match sealed {
            Sealed::Nothing => MaskingSeal::Files,
            Sealed::Broken => MaskingSeal::Lost,
            Sealed::Approved(approved) => {
                let Some(rules) = &approved.masking else {
                    return MaskingSeal::Files;
                };
                self.marks(approved)
                    .map_or(MaskingSeal::Lost, |marks| MaskingSeal::Approved {
                        rules: rules.clone(),
                        marks,
                    })
            }
        }
    }

    /// One look of the app at the files that say `current`: what they give agents beyond what was
    /// approved, for the question to the user. The first look seals the files as they are (a new
    /// profile, or the first with this version, once, without asking), and an edit that gives
    /// agents less is sealed at once. The marks are compared with `file_marks` only when
    /// `check_marks`: reading them is not for each start of the app.
    pub fn review(
        &self,
        current: &AgentPolicy,
        connections: &[SavedConnection],
        check_marks: bool,
        file_marks: &dyn Fn() -> Result<SealedMarks, String>,
    ) -> Vec<Loosening> {
        let sealed = self.read();
        let approved = match &sealed {
            Sealed::Nothing => {
                self.seal_first(current, file_marks);
                return Vec::new();
            }
            Sealed::Broken => return lost_seal(current, connections, check_marks, file_marks),
            Sealed::Approved(approved) => approved,
        };
        let tightened = current.within(approved);
        if approved.masking.is_none() {
            self.seal_first(&tightened, file_marks);
            return sealed.loosenings(current, connections);
        }
        let mut found = sealed.loosenings(current, connections);
        let mut marks_kept = None;
        if check_marks {
            let Some(marks) = self.marks(approved) else {
                return lost_seal(current, connections, check_marks, file_marks);
            };
            match file_marks() {
                Ok(file) => {
                    found.extend(marks.lifted_by(&file, connections));
                    marks_kept = Some(marks.within(&file)).filter(|kept| *kept != *marks);
                }
                Err(e) => eprintln!("agent policy: {e}"),
            }
        }
        let written = match marks_kept {
            Some(kept) => self.write_with_marks(&tightened, &kept),
            None if tightened != *approved => self.write(&tightened),
            None => Ok(()),
        };
        if let Err(e) = written {
            eprintln!("agent policy: {e}");
        }
        found
    }

    /// Seals `policy` with the marks of the catalog; one that cannot be read has its marks
    /// sealed at a later look, as additions.
    fn seal_first(
        &self,
        policy: &AgentPolicy,
        file_marks: &dyn Fn() -> Result<SealedMarks, String>,
    ) {
        let marks = file_marks().unwrap_or_else(|e| {
            eprintln!("agent policy: {e}");
            SealedMarks::default()
        });
        // Without a keychain there is nothing to compare with: agents get the files.
        if let Err(e) = self.write_with_marks(policy, &marks) {
            eprintln!("agent policy: {e}");
        }
    }

    /// What the user's "undo" leaves when the files say `current` and `file`: all that was
    /// approved. With a seal that no longer matches that is the app's defaults and the additions
    /// of the files, since nothing else can be told.
    #[must_use]
    pub fn undo(&self, current: &AgentPolicy, file: &SealedMarks) -> Undo {
        let mut sealed = self.read();
        if let Sealed::Approved(policy) = &sealed
            && self.marks(policy).is_none()
        {
            sealed = Sealed::Broken;
        }
        let approved: Option<Arc<SealedMarks>> = match &sealed {
            Sealed::Broken => Some(Arc::default()),
            Sealed::Approved(policy) if policy.masking.is_some() => self.marks(policy),
            _ => None,
        };
        Undo {
            policy: sealed.honored(current),
            marks: approved
                .as_ref()
                .map_or_else(|| file.clone(), |a| a.within(file)),
            restore: approved.map_or_else(Vec::new, |a| a.restoring(file)),
        }
    }
}

#[cfg(test)]
mod tests {
    use platypus_core::DriverKind;
    use platypus_sql::sensitive::NameRules;
    use platypus_store::{MemorySecrets, SecretStore};

    use super::*;
    use crate::policy::POLICY_FILE;

    fn key(object: &str, column: &str) -> CatalogKey {
        CatalogKey {
            database: String::new(),
            schema: "main".to_owned(),
            object: object.to_owned(),
            column: column.to_owned(),
        }
    }

    fn mark(column: &str, sensitive: bool) -> SensitiveColumn {
        SensitiveColumn {
            key: key("users", column),
            sensitive,
        }
    }

    /// A profile's seal and one connection, with what the files say kept here.
    struct Files {
        dir: tempfile::TempDir,
        seal: Seal,
        connection: SavedConnection,
        names: String,
        marks: Vec<SensitiveColumn>,
    }

    impl Files {
        fn new(names: &str, marks: Vec<SensitiveColumn>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
            let seal = Seal::with(dir.path(), secrets);
            Self {
                dir,
                seal,
                connection: SavedConnection::new("Users", DriverKind::Sqlite),
                names: names.to_owned(),
                marks,
            }
        }

        fn id(&self) -> Uuid {
            self.connection.id
        }

        fn policy(&self) -> AgentPolicy {
            let mut policy = AgentPolicy::current(std::slice::from_ref(&self.connection), None);
            policy.masking = Some(self.rules());
            policy
        }

        fn rules(&self) -> MaskingRules {
            MaskingRules {
                names: self.names.clone(),
                types: String::new(),
            }
        }

        fn catalog(&self) -> SealedMarks {
            SealedMarks::from_catalog([(self.id(), self.marks.clone())])
        }

        fn review(&self, check_marks: bool) -> Vec<Loosening> {
            self.seal.review(
                &self.policy(),
                std::slice::from_ref(&self.connection),
                check_marks,
                &|| Ok(self.catalog()),
            )
        }

        /// The user's "accept".
        fn accept(&self) {
            self.seal
                .write_with_marks(&self.policy(), &self.catalog())
                .unwrap();
        }

        /// What agents read now, seen through a new process.
        fn agents(&self) -> (MaskingRules, Vec<SensitiveColumn>) {
            let masking = self.seal.masking(&self.seal.read());
            let mut marks = masking.marks(self.id(), self.marks.clone());
            marks.sort_by(|a, b| a.key.cmp(&b.key));
            (masking.rules(self.rules()), marks)
        }

        fn masks_name(&self, name: &str) -> bool {
            NameRules::new(&self.agents().0.names).matches(name)
        }

        fn sealed_masking(&self) -> MaskingRules {
            let Sealed::Approved(policy) = self.seal.read() else {
                panic!("no approved policy");
            };
            policy.masking.unwrap()
        }
    }

    #[test]
    fn the_union_keeps_each_pattern_once_ignoring_case() {
        assert_eq!(union_of("a, b", "a, b"), "a, b");
        assert_eq!(union_of("a, b", "B,c"), "a, b, c");
        assert_eq!(union_of("", "x"), "x");
        assert_eq!(union_of("x", ""), "x");
        assert_eq!(union_of("b, a", "a,b"), "b, a");
        assert_eq!(removed_patterns("A", "a, *b*"), ["*b*"]);
    }

    #[test]
    fn a_mark_is_sensitive_when_either_says_and_not_sensitive_when_both_do() {
        let (yes, no) = (Some(true), Some(false));
        for (approved, file, honored) in [
            (yes, yes, yes),
            (yes, no, yes),
            (yes, None, yes),
            (no, yes, yes),
            (None, yes, yes),
            (no, no, no),
            (no, None, None),
            (None, no, None),
            (None, None, None),
        ] {
            assert_eq!(
                honored_mark(approved, file),
                honored,
                "{approved:?} {file:?}"
            );
        }
    }

    #[test]
    fn a_rule_removed_by_hand_still_masks_and_is_asked_about() {
        let mut files = Files::new("*email*, nick", vec![]);
        files.accept();
        files.names = "nick".to_owned();
        assert!(files.masks_name("email"), "the sealed rule still masks");
        assert_eq!(
            files.review(true),
            [Loosening::MaskingRules {
                by: MaskedBy::Name,
                patterns: vec!["*email*".to_owned()],
            }]
        );
        // Asking leaves the seal as it was, and narrowing a rule is removing it.
        assert!(files.sealed_masking().names.contains("*email*"));
        files.names = "email, nick".to_owned();
        assert!(files.masks_name("backup_email"));
        assert_eq!(files.review(true).len(), 1);
    }

    #[test]
    fn a_rule_added_by_hand_applies_at_once_and_is_sealed_at_the_next_look() {
        let mut files = Files::new("nick", vec![]);
        files.accept();
        files.names = "nick, *salary*".to_owned();
        assert!(files.masks_name("salary"), "before the app looks");
        assert_eq!(files.review(false), []);
        assert_eq!(files.sealed_masking().names, "nick, *salary*");
        // Sealed: taking it away is a removal now.
        files.names = "nick".to_owned();
        assert!(files.masks_name("salary"));
        assert_eq!(files.review(false).len(), 1);
    }

    #[test]
    fn a_mark_removed_or_lifted_by_hand_still_masks_and_is_asked_about() {
        let mut files = Files::new("", vec![mark("name", true), mark("id", false)]);
        files.accept();
        // Removed, turned to "not sensitive", and a "not sensitive" that was never approved.
        files.marks = vec![mark("nick", false), mark("email", true), mark("id", false)];
        files.marks.push(mark("name", false));
        let (_, marks) = files.agents();
        assert_eq!(
            marks,
            [mark("email", true), mark("id", false), mark("name", true)],
            "the sealed mark holds, the new not-sensitive does not count"
        );
        files.marks.retain(|m| m.key.column != "name");
        let (_, marks) = files.agents();
        assert!(marks.contains(&mark("name", true)), "{marks:?}");
        let found = files.review(true);
        let [
            Loosening::SensitiveMarks {
                count,
                columns,
                name,
                ..
            },
        ] = found.as_slice()
        else {
            panic!("{found:?}");
        };
        assert_eq!((*count, name.as_str()), (2, "Users"));
        assert_eq!(columns, &["main.users.name", "main.users.nick"]);
        // The addition of `email` is sealed at once; what waits stays out of the seal.
        let approved = files.seal.undo(&files.policy(), &files.catalog()).marks;
        assert_eq!(
            approved.0[&files.id()].get(&key("users", "email")),
            Some(&true)
        );
        assert_eq!(approved.0[&files.id()].get(&key("users", "nick")), None);
    }

    #[test]
    fn what_is_marked_in_the_catalog_when_the_catalog_is_gone_still_masks() {
        let mut files = Files::new("", vec![mark("name", true)]);
        files.accept();
        files.marks = Vec::new();
        assert_eq!(files.agents().1, [mark("name", true)]);
    }

    #[test]
    fn a_change_in_the_app_applies_as_saved_and_keeps_what_waits() {
        let mut rules = MaskingRules {
            names: "*email*, nick".to_owned(),
            types: "inet".to_owned(),
        };
        // The user takes `nick` out and adds `*phone*`.
        rules.apply_edit(MASK_NAMES, Some("*email*, nick"), Some("*email*, *phone*"));
        assert_eq!(rules.names, "*email*, *phone*");
        // An agent took `*email*` out of the file, and the user saves the field as it shows: that
        // approves what they typed, not what they were not asked about.
        rules.apply_edit(MASK_NAMES, Some("*phone*"), Some("*phone*, x"));
        assert_eq!(rules.names, "*email*, *phone*, x");
        // Back to the default, and another rule.
        rules.apply_edit(MASK_NAMES, Some("*email*, *phone*, x"), None);
        assert_eq!(pattern_set(&rules.names), pattern_set(DEFAULT_NAMES));
        rules.apply_edit(MASK_TYPES, Some("inet"), Some(""));
        assert_eq!(rules.types, "");
        rules.apply_edit("safety.other", Some("x"), Some("y"));
        assert_eq!(rules.types, "");
    }

    #[test]
    fn marks_changed_in_the_app_follow() {
        let id = Uuid::new_v4();
        let mut marks = SealedMarks::from_catalog([(id, vec![mark("email", true)])]);
        marks.set(id, &key("users", "nick"), Some(false));
        marks.set(id, &key("users", "email"), None);
        assert_eq!(marks.0[&id], Marks::from([(key("users", "nick"), false)]));

        marks.rename(id, &key("users", "nick"), &key("users", "alias"));
        assert_eq!(marks.0[&id], Marks::from([(key("users", "alias"), false)]));
        let people = CatalogKey {
            schema: "app".to_owned(),
            ..key("people", "")
        };
        marks.rename(id, &key("users", ""), &people);
        assert_eq!(
            marks.0[&id],
            Marks::from([(
                CatalogKey {
                    column: "alias".to_owned(),
                    ..people
                },
                false
            )])
        );
        marks.forget(id);
        assert_eq!(marks.count(), 0);
    }

    #[test]
    fn accepting_seals_the_files_as_they_are() {
        let mut files = Files::new("*email*", vec![mark("name", true)]);
        files.accept();
        files.names = "nick".to_owned();
        files.marks = vec![mark("email", false)];
        assert_ne!(files.review(true), []);
        files.accept();
        assert_eq!(files.review(true), []);
        assert!(!files.masks_name("email"));
        assert_eq!(files.agents().1, [mark("email", false)]);
    }

    #[test]
    fn undoing_puts_back_what_was_approved_and_keeps_the_additions() {
        let mut files = Files::new("*email*", vec![mark("name", true), mark("id", false)]);
        files.accept();
        files.names = "nick".to_owned();
        files.marks = vec![mark("nick", true), mark("email", false)];
        assert_ne!(files.review(true), []);

        let undo = files.seal.undo(&files.policy(), &files.catalog());
        assert_eq!(undo.policy.masking.as_ref().unwrap().names, "nick, *email*");
        let mut restore: Vec<_> = undo
            .restore
            .iter()
            .map(|(_, key, mark)| (key.column.as_str(), *mark))
            .collect();
        restore.sort_unstable();
        // `name` comes back, and the "not sensitive" nobody approved goes. `id`, a "not sensitive"
        // taken out by hand, only masks more: it stays out.
        assert_eq!(restore, [("email", None), ("name", Some(true))]);

        // What the app then does: the files put back, and sealed so.
        files.names = undo.policy.masking.as_ref().unwrap().names.clone();
        files.marks = vec![mark("nick", true), mark("name", true)];
        files
            .seal
            .write_with_marks(&undo.policy, &undo.marks)
            .unwrap();
        assert_eq!(files.review(true), []);
        assert_eq!(files.agents().1, [mark("name", true), mark("nick", true)]);
    }

    #[test]
    fn without_a_seal_the_files_are_taken_as_they_are_and_sealed_once() {
        let files = Files::new("", vec![mark("email", false)]);
        assert_eq!(files.seal.read(), Sealed::Nothing);
        assert!(!files.masks_name("email"));
        assert_eq!(files.agents().1, [mark("email", false)]);
        assert_eq!(files.review(false), []);
        // The first look seals them, the marks too, even when not asked to compare them.
        assert_eq!(files.sealed_masking().names, "");
        assert_eq!(files.agents().1, [mark("email", false)]);
        assert_eq!(files.review(true), []);
    }

    #[test]
    fn a_seal_that_no_longer_matches_masks_every_value_until_the_user_answers() {
        let mut files = Files::new("", vec![mark("name", true), mark("email", false)]);
        files.accept();
        // An agent takes the rules and marks out, and removes the policy that recorded them.
        files.marks = vec![mark("nick", false), mark("id", true)];
        std::fs::remove_file(files.dir.path().join(POLICY_FILE)).unwrap();
        assert_eq!(files.seal.read(), Sealed::Broken);
        let (rules, marks) = files.agents();
        assert!(NameRules::new(&rules.names).matches("anything"));
        assert!(NameRules::new(&rules.types).matches("text"));
        assert_eq!(
            marks,
            [mark("id", true)],
            "a not-sensitive mark lifts nothing"
        );

        // The question is asked even with nothing else to ask about.
        assert_eq!(files.review(false)[0], Loosening::MaskingLost);
        let mut defaults = Files::new(DEFAULT_NAMES, Vec::new());
        defaults.accept();
        std::fs::remove_file(defaults.dir.path().join(POLICY_FILE)).unwrap();
        assert_eq!(defaults.review(false), [Loosening::MaskingLost]);
        defaults.names = String::new();
        assert_eq!(
            defaults.review(false).len(),
            2,
            "and the rules of the defaults it lacks"
        );
        assert!(files.review(true).contains(&Loosening::MaskingLost));
        assert_eq!(
            files.seal.read(),
            Sealed::Broken,
            "and waits for the answer"
        );

        // Undo: the defaults and what the files add, since nothing else can be told.
        let undo = files.seal.undo(&files.policy(), &files.catalog());
        assert_eq!(undo.policy.masking.as_ref().unwrap().names, DEFAULT_NAMES);
        assert_eq!(undo.restore.len(), 1);
        assert_eq!(undo.restore[0].1, key("users", "nick"));
        assert_eq!(undo.marks.count(), 1);

        // Accept: back to what the files say.
        files.accept();
        assert!(!files.masks_name("anything"));
    }

    #[test]
    fn a_marks_file_that_no_longer_matches_is_a_lost_seal_for_a_new_process() {
        let files = Files::new("", vec![mark("name", true)]);
        files.accept();
        let path = files.dir.path().join(MARKS_FILE);
        let sealed = files.seal.read();
        assert!(matches!(
            files.seal.masking(&sealed),
            MaskingSeal::Approved { .. }
        ));
        for forged in [Some("{}"), None] {
            match forged {
                Some(text) => std::fs::write(&path, text).unwrap(),
                None => std::fs::remove_file(&path).unwrap(),
            }
            // The process that read it goes on with what it verified, reading nothing.
            assert!(matches!(
                files.seal.masking(&sealed),
                MaskingSeal::Approved { .. }
            ));
            let new = files.seal.without_loaded_marks();
            assert_eq!(new.masking(&sealed), MaskingSeal::Lost);
            let found = new.review(
                &files.policy(),
                std::slice::from_ref(&files.connection),
                true,
                &|| Ok(files.catalog()),
            );
            assert!(found.contains(&Loosening::MaskingLost), "{found:?}");
            let undo = new.undo(&files.policy(), &files.catalog());
            assert_eq!(undo.policy.masking.as_ref().unwrap().names, DEFAULT_NAMES);
        }
    }

    #[test]
    fn a_policy_of_an_older_version_takes_the_files_as_the_first_seal_once() {
        let mut files = Files::new("nick", vec![mark("name", true), mark("id", false)]);
        let mut older = files.policy();
        older.masking = None;
        files.seal.write(&older).unwrap();
        let bytes = std::fs::read_to_string(files.dir.path().join(POLICY_FILE)).unwrap();
        assert!(
            !bytes.contains("masking") && !bytes.contains("marks"),
            "{bytes}"
        );
        // Until the app looks, agents get the files.
        assert!(!files.masks_name("email"));
        assert_eq!(files.agents().1, [mark("id", false), mark("name", true)]);

        assert_eq!(files.review(false), [], "without asking");
        assert_eq!(files.sealed_masking().names, "nick");
        // From now on the seal holds.
        files.names = String::new();
        files.marks = Vec::new();
        assert!(files.masks_name("nick"));
        assert_eq!(files.agents().1, [mark("name", true)]);
        assert_eq!(files.review(true).len(), 2);
    }

    #[test]
    fn a_look_without_the_marks_reads_neither_them_nor_their_file() {
        let files = Files::new("x", vec![mark("name", true)]);
        files.accept();
        std::fs::remove_file(files.dir.path().join(MARKS_FILE)).unwrap();
        let found = files.seal.review(
            &files.policy(),
            std::slice::from_ref(&files.connection),
            false,
            &|| panic!("the marks are not for this look"),
        );
        assert_eq!(found, []);
    }

    #[test]
    fn the_seal_stays_small_with_10_000_marks_and_the_check_reads_no_more() {
        let id = Uuid::new_v4();
        let marks: Vec<SensitiveColumn> = (0..10_000)
            .map(|i| SensitiveColumn {
                key: CatalogKey {
                    database: String::new(),
                    schema: "public".to_owned(),
                    object: format!("customer_table_{:04}", i / 10),
                    column: format!("personal_column_{:02}", i % 10),
                },
                sensitive: i % 7 != 0,
            })
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let seal = Seal::with(dir.path(), secrets);
        let mut connection = SavedConnection::new("Big", DriverKind::Sqlite);
        connection.id = id;
        let policy = AgentPolicy::current(std::slice::from_ref(&connection), None);
        let sealed_marks = SealedMarks::from_catalog([(id, marks)]);
        assert_eq!(sealed_marks.count(), 10_000);
        seal.write_with_marks(&policy, &sealed_marks).unwrap();
        let size = |file: &str| std::fs::metadata(dir.path().join(file)).unwrap().len();
        let (policy_bytes, marks_bytes) = (size(POLICY_FILE), size(MARKS_FILE));
        println!(
            "with 10,000 marks: {POLICY_FILE} {policy_bytes} bytes, {MARKS_FILE} {marks_bytes} bytes"
        );
        assert!(policy_bytes < 1024, "{policy_bytes}");
        assert!(marks_bytes < 400 * 1024, "{marks_bytes}");
        // What a new process reads back is what was sealed.
        let Sealed::Approved(read) = seal.read() else {
            panic!("no approved policy");
        };
        let new = seal.without_loaded_marks();
        assert_eq!(*new.marks(&read).unwrap(), sealed_marks);
    }
}
