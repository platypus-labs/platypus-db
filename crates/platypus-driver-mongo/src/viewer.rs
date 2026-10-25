//! The data viewer's rewrites of a `find` statement: a header sort replaces its `.sort(…)`
//! and "Count rows" counts its filter. The rest of the text stays as written.

use std::fmt::Write;

use crate::shell::{self, CollectionOp, Command};

/// Where the parts of `<target>.find(<args>)<chain>` are, as byte offsets.
struct Find {
    /// Start of `.find(`.
    dot: usize,
    /// The first argument, the filter, as written.
    filter: (usize, usize),
    /// The byte after the closing parenthesis.
    end: usize,
}

/// The text of `src` up to `i`, skipping strings and comments, tracks the nesting of the
/// brackets: `step` is called with each byte offset outside them and its depth.
fn scan(src: &str, mut step: impl FnMut(usize, usize) -> bool) {
    let bytes = src.as_bytes();
    let (mut i, mut depth) = (0, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |n| i + 2 + n + 1);
            }
            b'(' | b'[' | b'{' => {
                if !step(i, depth) {
                    return;
                }
                depth += 1;
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {
                if !step(i, depth) {
                    return;
                }
            }
        }
        i += 1;
    }
}

/// The offset of the `)` that closes the `(` at `open`.
fn close(src: &str, open: usize) -> Option<usize> {
    let tail = &src[open..];
    let bytes = tail.as_bytes();
    let (mut depth, mut i) = (0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The `.find(…)` call of a statement that parses as a plain `find`.
fn find(src: &str) -> Option<Find> {
    let Ok(Command::Collection {
        op: CollectionOp::Find(f),
        ..
    }) = shell::parse(src)
    else {
        return None;
    };
    if f.one {
        return None;
    }
    let mut dot = None;
    scan(src, |i, depth| {
        let call = depth == 0
            && src[i..].starts_with(".find")
            && src[i + 5..].trim_start().starts_with('(');
        if call {
            dot = Some(i);
        }
        !call
    });
    let dot = dot?;
    let open = dot + 5 + src[dot + 5..].find('(')?;
    let end = close(src, open)? + 1;
    // The filter is the first argument: up to a comma outside brackets.
    let args = &src[open + 1..end - 1];
    let mut cut = args.len();
    scan(args, |i, depth| {
        if depth == 0 && args.as_bytes()[i] == b',' {
            cut = i;
            return false;
        }
        true
    });
    Some(Find {
        dot,
        filter: (open + 1, open + 1 + cut),
        end,
    })
}

/// A field name as a document key.
fn key(field: &str) -> String {
    let bare = field.chars().enumerate().all(|(i, c)| {
        c == '_' || c == '$' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
    });
    if bare && !field.is_empty() {
        field.to_owned()
    } else {
        format!("\"{}\"", field.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Whether the statement is a `find` whose sort the viewer can change.
#[must_use]
pub fn sortable(src: &str) -> bool {
    find(src).is_some()
}

/// `src` sorted by `sort` (`true`: descending), or with no sort when `None`: any `.sort(…)`
/// of the chain is replaced, the filter and the other modifiers stay.
#[must_use]
pub fn set_sort(src: &str, sort: Option<(&str, bool)>) -> Option<String> {
    let f = find(src)?;
    let mut out = src[..f.end].to_owned();
    if let Some((field, desc)) = sort {
        let direction = if desc { -1 } else { 1 };
        let _ = write!(out, ".sort({{ {}: {direction} }})", key(field));
    }
    // The rest of the chain, without its `.sort(…)` calls.
    let rest = &src[f.end..];
    let mut at = 0;
    let mut cuts = Vec::new();
    scan(rest, |i, depth| {
        if depth == 0
            && i >= at
            && rest[i..].starts_with(".sort")
            && rest[i + 5..].trim_start().starts_with('(')
            && let Some(open) = rest[i + 5..].find('(')
            && let Some(end) = close(rest, i + 5 + open)
        {
            cuts.push((i, end + 1));
            at = end + 1;
        }
        true
    });
    let mut from = 0;
    for (start, end) in cuts {
        out.push_str(&rest[from..start]);
        from = end;
    }
    out.push_str(&rest[from..]);
    Some(out)
}

/// `<collection>.countDocuments(<filter>)` for a `find` statement.
#[must_use]
pub fn count(src: &str) -> Option<String> {
    let f = find(src)?;
    let filter = src[f.filter.0..f.filter.1].trim();
    let filter = if filter.is_empty() { "{}" } else { filter };
    Some(format!(
        "{}.countDocuments({filter})",
        src[..f.dot].trim_start()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_and_keeps_the_filter() {
        assert_eq!(
            set_sort(
                "db.customers.find({ country: 'IT' })",
                Some(("name", false))
            )
            .unwrap(),
            "db.customers.find({ country: 'IT' }).sort({ name: 1 })"
        );
        assert_eq!(
            set_sort(
                "db.c.find({ a: { $gt: 1 } }, { a: 1 }).sort({ b: 1 }).limit(5)",
                Some(("a b", true))
            )
            .unwrap(),
            "db.c.find({ a: { $gt: 1 } }, { a: 1 }).sort({ \"a b\": -1 }).limit(5)"
        );
        assert_eq!(
            set_sort("db.c.find({}).sort({ b: 1 })", None).unwrap(),
            "db.c.find({})"
        );
    }

    #[test]
    fn leaves_strings_alone() {
        assert_eq!(
            set_sort("db.c.find({ s: '.sort({a:1})' })", Some(("_id", false))).unwrap(),
            "db.c.find({ s: '.sort({a:1})' }).sort({ _id: 1 })"
        );
    }

    #[test]
    fn counts_the_filter() {
        assert_eq!(
            count("db.c.find({ a: 1 }).sort({ a: 1 })").unwrap(),
            "db.c.countDocuments({ a: 1 })"
        );
        assert_eq!(
            count("db.c.find({ a: 1 }, { b: 1 })").unwrap(),
            "db.c.countDocuments({ a: 1 })"
        );
        assert_eq!(count("db.c.find()").unwrap(), "db.c.countDocuments({})");
        assert_eq!(count("db.c.find({})").unwrap(), "db.c.countDocuments({})");
    }

    #[test]
    fn gives_up_on_other_statements() {
        for src in [
            "db.c.findOne({})",
            "db.c.aggregate([])",
            "db.c.deleteMany({})",
            "show dbs",
            "db.c.find(",
        ] {
            assert!(!sortable(src), "{src}");
            assert_eq!(set_sort(src, None), None, "{src}");
            assert_eq!(count(src), None, "{src}");
        }
    }
}
