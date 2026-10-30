//! The merge: a pure function of this machine's items, the file's items and the state at the
//! last sync. No clocks are read and nothing is written here, so every rule can be tested.
//!
//! Per item, with `base` the fingerprint both sides had at the last sync:
//!
//! - Same content on both sides: nothing to do.
//! - Present on one side only: it is new there (or its tombstone was pruned): both sides get it.
//! - Changed on one side since `base`: that side wins, whatever the clocks say.
//! - Changed on both sides: the later change (then the machine name, to break a tie the same
//!   way everywhere) keeps the item's name. For scripts and connections the other version is
//!   kept too, as a copy named after the machine that made it. A deletion against an edit
//!   keeps the edit. Everything is reported as a [`Conflict`].

use std::collections::BTreeMap;

use ring::digest::{SHA256, digest};
use serde_json::Value;
use uuid::Uuid;

use crate::model::{BaseEntry, CONNECTION, Item, SCRIPT};

/// How long a tombstone is kept: a machine offline for longer brings back what was deleted.
pub const TOMBSTONE_TTL_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// What to do to this machine's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Create or replace the item.
    Put { key: String, item: Item },
    /// The item was deleted elsewhere.
    Remove { key: String },
}

impl Change {
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Put { key, .. } | Self::Remove { key } => key,
        }
    }
}

/// Why an item needed a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both sides changed it since the last sync.
    BothEdited,
    /// One side deleted it, the other changed it: the change is kept.
    DeletedAndEdited,
}

/// An item that changed on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub key: String,
    pub kind: ConflictKind,
    /// Where the version that lost was kept, when it was.
    pub copy: Option<String>,
    /// The machine that made the version that lost (or deleted the item).
    pub other_machine: String,
}

/// The result of a merge.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Merged {
    /// What the file and this machine hold from now on.
    pub items: BTreeMap<String, Item>,
    /// What this machine must do to its data to match `items`.
    pub apply: Vec<Change>,
    pub conflicts: Vec<Conflict>,
}

/// Who and when, for the merge.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub now: i64,
    pub machine: &'a str,
}

/// Merges `local` (this machine's items, tombstones included, see
/// `collect::local_items`) with `remote` (the file's).
#[must_use]
pub fn merge(
    local: &BTreeMap<String, Item>,
    remote: &BTreeMap<String, Item>,
    base: &BTreeMap<String, BaseEntry>,
    ctx: Context<'_>,
) -> Merged {
    let mut out = Merged::default();
    let keys: std::collections::BTreeSet<&String> = local.keys().chain(remote.keys()).collect();
    for key in keys {
        let known = base.get(key).map(|b| b.fingerprint.as_str());
        match (local.get(key), remote.get(key)) {
            (Some(l), None) => {
                out.items.insert(key.clone(), l.clone());
            }
            (None, Some(r)) => take_remote(&mut out, key, r),
            (Some(l), Some(r)) => both(&mut out, key, l, r, known),
            (None, None) => {}
        }
    }
    prune(&mut out.items, ctx.now);
    out
}

/// The remote item is the one to keep; the machine follows it.
fn take_remote(out: &mut Merged, key: &str, r: &Item) {
    out.items.insert(key.to_owned(), r.clone());
    if r.deleted {
        // Nothing here to delete if the machine never had the item; removing is harmless.
        out.apply.push(Change::Remove {
            key: key.to_owned(),
        });
    } else {
        out.apply.push(Change::Put {
            key: key.to_owned(),
            item: r.clone(),
        });
    }
}

fn both(out: &mut Merged, key: &str, l: &Item, r: &Item, known: Option<&str>) {
    let (fl, fr) = (l.fingerprint(), r.fingerprint());
    if fl == fr {
        // The same content: keep the later stamp, so every machine ends with the same file.
        let kept = if (r.modified, &r.by) > (l.modified, &l.by) {
            r
        } else {
            l
        };
        out.items.insert(key.to_owned(), kept.clone());
        return;
    }
    let local_changed = known != Some(fl.as_str());
    let remote_changed = known != Some(fr.as_str());
    match (local_changed, remote_changed) {
        (true, false) => {
            out.items.insert(key.to_owned(), l.clone());
        }
        (false, true) => take_remote(out, key, r),
        _ => conflict(out, key, l, r),
    }
}

/// Both sides changed the item.
fn conflict(out: &mut Merged, key: &str, l: &Item, r: &Item) {
    if l.deleted != r.deleted {
        let deleter = if l.deleted { l } else { r };
        if l.deleted {
            take_remote(out, key, r);
        } else {
            out.items.insert(key.to_owned(), l.clone());
        }
        out.conflicts.push(Conflict {
            key: key.to_owned(),
            kind: ConflictKind::DeletedAndEdited,
            copy: None,
            other_machine: deleter.by.clone(),
        });
        return;
    }
    let remote_wins = (r.modified, &r.by) > (l.modified, &l.by);
    let (winner, loser) = if remote_wins { (r, l) } else { (l, r) };
    out.items.insert(key.to_owned(), winner.clone());
    if remote_wins {
        out.apply.push(Change::Put {
            key: key.to_owned(),
            item: r.clone(),
        });
    }
    let copy = copy_of(key, loser).map(|(copy_key, copy_item)| {
        let copy_key = free_key(&out.items, &copy_key, &copy_item);
        if !out.items.contains_key(&copy_key) {
            out.items.insert(copy_key.clone(), copy_item.clone());
            out.apply.push(Change::Put {
                key: copy_key.clone(),
                item: copy_item,
            });
        }
        copy_key
    });
    out.conflicts.push(Conflict {
        key: key.to_owned(),
        kind: ConflictKind::BothEdited,
        copy,
        other_machine: loser.by.clone(),
    });
}

/// A key not used by another content: `key` itself when free or already holding `item`.
fn free_key(items: &BTreeMap<String, Item>, key: &str, item: &Item) -> String {
    let mut candidate = key.to_owned();
    let mut n = 2;
    while items
        .get(&candidate)
        .is_some_and(|x| x.fingerprint() != item.fingerprint())
    {
        candidate = match key.strip_suffix(".sql") {
            Some(stem) => format!("{stem} {n}.sql"),
            None => format!("{key}-{n}"),
        };
        n += 1;
    }
    candidate
}

/// Drops the tombstones older than the time to live.
fn prune(items: &mut BTreeMap<String, Item>, now: i64) {
    items.retain(|_, i| !i.deleted || now - i.modified < TOMBSTONE_TTL_MS);
}

/// Characters a file name cannot hold, and spaces around.
fn clean_machine(machine: &str) -> String {
    let name: String = machine
        .chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()
        })
        .collect();
    let name = name.trim();
    if name.is_empty() {
        "other".to_owned()
    } else {
        name.to_owned()
    }
}

/// The copy that keeps a losing version: scripts get `name (conflict <machine>).sql`,
/// connections a new id (the same on every machine for the same version) and a name suffix.
/// Other kinds are last-writer-wins and keep no copy.
fn copy_of(key: &str, loser: &Item) -> Option<(String, Item)> {
    let machine = clean_machine(&loser.by);
    if let Some(path) = key.strip_prefix(SCRIPT) {
        let stem = path.strip_suffix(".sql").unwrap_or(path);
        let copy = format!("{SCRIPT}{stem} (conflict {machine}).sql");
        return Some((copy, loser.clone()));
    }
    let id = key.strip_prefix(CONNECTION)?;
    let hash = digest(&SHA256, format!("{id}/{}", loser.fingerprint()).as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash.as_ref()[..16]);
    let new_id = uuid::Builder::from_random_bytes(bytes).into_uuid();
    let mut value = loser.value.clone();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Connection")
        .to_owned();
    value["id"] = Value::String(new_id.to_string());
    value["name"] = Value::String(format!("{name} (conflict {machine})"));
    let item = Item {
        value,
        ..loser.clone()
    };
    Some((format!("{CONNECTION}{new_id}"), item))
}

/// Whether `key` names the connection that `copy_of` made, for tests and the UI.
#[must_use]
pub fn is_connection_key(key: &str) -> bool {
    key.strip_prefix(CONNECTION)
        .is_some_and(|id| Uuid::parse_str(id).is_ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: i64 = 1_000_000_000_000;

    fn ctx() -> Context<'static> {
        Context {
            now: NOW,
            machine: "A",
        }
    }

    fn live(v: Value, at: i64, by: &str) -> Item {
        Item::live(v, at, by)
    }

    fn items(list: &[(&str, Item)]) -> BTreeMap<String, Item> {
        list.iter()
            .map(|(k, i)| ((*k).to_owned(), i.clone()))
            .collect()
    }

    fn base(list: &[(&str, &Item)]) -> BTreeMap<String, BaseEntry> {
        list.iter()
            .map(|(k, i)| ((*k).to_owned(), BaseEntry::of(i)))
            .collect()
    }

    #[test]
    fn identical_content_needs_nothing() {
        let a = live(json!(1), 1, "A");
        let b = live(json!(1), 9, "B");
        let m = merge(
            &items(&[("setting/x", a)]),
            &items(&[("setting/x", b.clone())]),
            &BTreeMap::new(),
            ctx(),
        );
        assert!(m.apply.is_empty() && m.conflicts.is_empty());
        assert_eq!(m.items["setting/x"], b, "the later stamp is kept");
    }

    #[test]
    fn new_items_travel_both_ways() {
        let l = live(json!("local"), 1, "A");
        let r = live(json!("remote"), 2, "B");
        let m = merge(
            &items(&[("setting/l", l.clone())]),
            &items(&[("setting/r", r.clone())]),
            &BTreeMap::new(),
            ctx(),
        );
        assert_eq!(m.items.len(), 2);
        assert_eq!(
            m.apply,
            [Change::Put {
                key: "setting/r".into(),
                item: r
            }]
        );
    }

    #[test]
    fn a_change_on_one_side_wins_whatever_the_clocks_say() {
        let old = live(json!("old"), 100, "A");
        // The remote edit has an earlier stamp (a slow clock) but is the only change.
        let remote = live(json!("new"), 50, "B");
        let m = merge(
            &items(&[("setting/x", old.clone())]),
            &items(&[("setting/x", remote.clone())]),
            &base(&[("setting/x", &old)]),
            ctx(),
        );
        assert_eq!(m.conflicts, []);
        assert_eq!(m.items["setting/x"], remote);
        assert_eq!(
            m.apply,
            [Change::Put {
                key: "setting/x".into(),
                item: remote
            }]
        );
        // And the other way: only the local side changed.
        let m = merge(
            &items(&[("setting/x", live(json!("mine"), 1, "A"))]),
            &items(&[("setting/x", old.clone())]),
            &base(&[("setting/x", &old)]),
            ctx(),
        );
        assert!(m.apply.is_empty() && m.conflicts.is_empty());
        assert_eq!(m.items["setting/x"].value, json!("mine"));
    }

    #[test]
    fn deletions_travel_as_tombstones() {
        let old = live(json!(1), 100, "A");
        let gone = Item::tombstone(NOW - 200, "B");
        let m = merge(
            &items(&[("connection/c", old.clone())]),
            &items(&[("connection/c", gone.clone())]),
            &base(&[("connection/c", &old)]),
            ctx(),
        );
        assert_eq!(
            m.apply,
            [Change::Remove {
                key: "connection/c".into()
            }]
        );
        assert!(m.items["connection/c"].deleted);
        // Deleted here, untouched there: the tombstone goes to the file, nothing is applied.
        let m = merge(
            &items(&[("connection/c", Item::tombstone(NOW - 300, "A"))]),
            &items(&[("connection/c", old.clone())]),
            &base(&[("connection/c", &old)]),
            ctx(),
        );
        assert!(m.apply.is_empty() && m.items["connection/c"].deleted);
    }

    #[test]
    fn old_tombstones_are_dropped() {
        let t = Item::tombstone(NOW - TOMBSTONE_TTL_MS - 1, "A");
        let fresh = Item::tombstone(NOW - 1000, "A");
        let m = merge(
            &items(&[("setting/a", t), ("setting/b", fresh)]),
            &BTreeMap::new(),
            &BTreeMap::new(),
            ctx(),
        );
        assert!(!m.items.contains_key("setting/a"));
        assert!(m.items.contains_key("setting/b"));
    }

    #[test]
    fn an_edit_on_both_sides_keeps_both_scripts() {
        let old = live(json!({"text": "select 1"}), 100, "A");
        let local = live(json!({"text": "select 2"}), 300, "A");
        let remote = live(json!({"text": "select 3"}), 200, "Work PC");
        let key = "script/all/reports/m.sql";
        let m = merge(
            &items(&[(key, local.clone())]),
            &items(&[(key, remote.clone())]),
            &base(&[(key, &old)]),
            ctx(),
        );
        assert_eq!(m.items[key], local, "the later change keeps the name");
        let copy = "script/all/reports/m (conflict Work PC).sql";
        assert_eq!(m.items[copy], remote);
        assert_eq!(
            m.apply,
            [Change::Put {
                key: copy.into(),
                item: remote
            }]
        );
        assert_eq!(
            m.conflicts,
            [Conflict {
                key: key.into(),
                kind: ConflictKind::BothEdited,
                copy: Some(copy.into()),
                other_machine: "Work PC".into(),
            }]
        );
    }

    #[test]
    fn when_the_remote_edit_is_later_it_replaces_the_script_here() {
        let old = live(json!("a"), 100, "A");
        let local = live(json!("b"), 150, "A");
        let remote = live(json!("c"), 400, "B");
        let key = "script/p/x.sql";
        let m = merge(
            &items(&[(key, local.clone())]),
            &items(&[(key, remote.clone())]),
            &base(&[(key, &old)]),
            ctx(),
        );
        assert_eq!(m.items[key], remote);
        assert_eq!(m.items["script/p/x (conflict A).sql"], local);
        assert_eq!(m.apply.len(), 2);
        assert!(m.apply.contains(&Change::Put {
            key: key.into(),
            item: remote
        }));
    }

    #[test]
    fn a_connection_conflict_makes_a_renamed_duplicate_with_a_stable_id() {
        let id = "11111111-1111-4111-8111-111111111111";
        let key = format!("connection/{id}");
        let old = live(json!({"id": id, "name": "Prod", "host": "h"}), 1, "A");
        let local = live(json!({"id": id, "name": "Prod", "host": "h1"}), 5, "A");
        let remote = live(json!({"id": id, "name": "Prod", "host": "h2"}), 9, "B");
        let a = merge(
            &items(&[(&key, local.clone())]),
            &items(&[(&key, remote.clone())]),
            &base(&[(&key, &old)]),
            ctx(),
        );
        let copies: Vec<_> = a.items.iter().filter(|(k, _)| **k != key).collect();
        assert_eq!(copies.len(), 1);
        let (copy_key, copy) = copies[0];
        assert!(is_connection_key(copy_key));
        assert_eq!(copy.value["name"], "Prod (conflict A)");
        assert_eq!(copy.value["host"], "h1");
        assert_eq!(
            copy.value["id"].as_str().unwrap(),
            copy_key.strip_prefix("connection/").unwrap()
        );
        // The other machine, merging the same two versions, makes the same copy: no duplicate.
        let b = merge(
            &items(&[(&key, remote.clone())]),
            &items(&[(&key, local.clone())]),
            &base(&[(&key, &old)]),
            Context {
                now: NOW,
                machine: "B",
            },
        );
        assert_eq!(a.items, b.items);
    }

    #[test]
    fn a_deletion_against_an_edit_keeps_the_edit() {
        let old = live(json!("a"), 100, "A");
        let edited = live(json!("b"), 200, "A");
        let gone = Item::tombstone(900, "B");
        let key = "setting/k";
        let m = merge(
            &items(&[(key, edited.clone())]),
            &items(&[(key, gone.clone())]),
            &base(&[(key, &old)]),
            ctx(),
        );
        assert_eq!(m.items[key], edited);
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].kind, ConflictKind::DeletedAndEdited);
        // Deleted here, edited there: the edit comes back here.
        let m = merge(
            &items(&[(key, Item::tombstone(900, "A"))]),
            &items(&[(key, edited.clone())]),
            &base(&[(key, &old)]),
            ctx(),
        );
        assert_eq!(m.items[key], edited);
        assert_eq!(
            m.apply,
            [Change::Put {
                key: key.into(),
                item: edited
            }]
        );
    }

    #[test]
    fn other_kinds_are_last_writer_wins_without_a_copy() {
        let old = live(json!(1), 1, "A");
        let key = "project/22222222-2222-4222-8222-222222222222";
        let m = merge(
            &items(&[(key, live(json!(2), 5, "A"))]),
            &items(&[(key, live(json!(3), 4, "B"))]),
            &base(&[(key, &old)]),
            ctx(),
        );
        assert_eq!(m.items.len(), 1);
        assert_eq!(m.items[key].value, json!(2));
        assert_eq!(m.conflicts[0].copy, None);
    }

    #[test]
    fn a_tie_is_broken_the_same_way_on_both_machines() {
        let old = live(json!(0), 1, "A");
        let a = live(json!("a"), 50, "A");
        let b = live(json!("b"), 50, "B");
        let key = "setting/t";
        let x = merge(
            &items(&[(key, a.clone())]),
            &items(&[(key, b.clone())]),
            &base(&[(key, &old)]),
            ctx(),
        );
        let y = merge(
            &items(&[(key, b)]),
            &items(&[(key, a)]),
            &base(&[(key, &old)]),
            ctx(),
        );
        assert_eq!(x.items, y.items);
    }

    #[test]
    fn a_first_sync_of_differing_items_is_a_conflict_not_a_loss() {
        // No base: neither side can claim the other is stale.
        let key = "script/all/a.sql";
        let m = merge(
            &items(&[(key, live(json!("x"), 5, "A"))]),
            &items(&[(key, live(json!("y"), 4, "B"))]),
            &BTreeMap::new(),
            ctx(),
        );
        assert_eq!(m.items.len(), 2);
    }

    #[test]
    fn merging_twice_is_stable() {
        let old = live(json!("a"), 100, "A");
        let key = "script/all/s.sql";
        let first = merge(
            &items(&[(key, live(json!("b"), 300, "A"))]),
            &items(&[(key, live(json!("c"), 200, "B"))]),
            &base(&[(key, &old)]),
            ctx(),
        );
        // Both machines now hold `first.items`; the state records it.
        let state: BTreeMap<String, BaseEntry> = first
            .items
            .iter()
            .map(|(k, i)| (k.clone(), BaseEntry::of(i)))
            .collect();
        let second = merge(&first.items, &first.items, &state, ctx());
        assert_eq!(second.items, first.items);
        assert!(second.apply.is_empty() && second.conflicts.is_empty());
    }
}
