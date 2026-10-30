//! Parsers for the machine-readable output of `git status --porcelain=v2 -z --branch` and of
//! `git diff`. They are pure so they can be tested on fixed outputs.

use serde::Serialize;

/// Where the checked-out branch stands against its upstream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Branch {
    /// `None` when HEAD is detached.
    pub name: Option<String>,
    /// The tracked branch, such as `origin/main`; `None` when there is none.
    pub upstream: Option<String>,
    /// Commits here that the upstream lacks.
    pub ahead: u32,
    /// Commits on the upstream that are not here.
    pub behind: u32,
    /// False in a repository with no commit yet.
    pub has_commits: bool,
}

/// One changed file of the working tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileStatus {
    /// Relative to the repository, with `/`.
    pub path: String,
    /// The old path of a renamed file.
    pub orig_path: Option<String>,
    /// Staged state (`X` of the porcelain output), `.` for none.
    pub index: char,
    /// Unstaged state (`Y`), `.` for none; `?` for an untracked file.
    pub worktree: char,
    /// What the Scripts panel shows: `M`odified, `A`dded, `D`eleted, `R`enamed, `?` untracked,
    /// `U` unmerged (a conflict).
    pub badge: char,
}

/// The parsed `git status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: Branch,
    pub files: Vec<FileStatus>,
}

fn badge(kind: char, x: char, y: char) -> char {
    match kind {
        '?' => '?',
        'u' => 'U',
        _ if x == 'D' || y == 'D' => 'D',
        _ if x == 'A' || y == 'A' => 'A',
        _ if x == 'R' || y == 'R' => 'R',
        _ => 'M',
    }
}

fn parse_header(line: &str, branch: &mut Branch) {
    if let Some(oid) = line.strip_prefix("branch.oid ") {
        branch.has_commits = oid != "(initial)";
    } else if let Some(head) = line.strip_prefix("branch.head ") {
        branch.name = (head != "(detached)").then(|| head.to_owned());
    } else if let Some(up) = line.strip_prefix("branch.upstream ") {
        branch.upstream = Some(up.to_owned());
    } else if let Some(ab) = line.strip_prefix("branch.ab ") {
        let mut it = ab.split(' ');
        let num = |s: Option<&str>, sign: char| {
            s.and_then(|s| s.strip_prefix(sign))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        branch.ahead = num(it.next(), '+');
        branch.behind = num(it.next(), '-');
    }
}

/// Parses `git status --porcelain=v2 -z --branch --untracked-files=all`. Unknown or
/// malformed records are skipped, so a newer Git never breaks the panel.
#[must_use]
pub fn parse_status(out: &str) -> Status {
    let mut status = Status::default();
    let mut records = out.split('\0');
    while let Some(rec) = records.next() {
        if let Some(header) = rec.strip_prefix("# ") {
            parse_header(header, &mut status.branch);
            continue;
        }
        let mut chars = rec.chars();
        let Some(kind) = chars.next() else { continue };
        // Field counts, with the path last: ordinary 9, rename/copy 10, unmerged 11.
        let (fields, has_orig) = match kind {
            '1' => (9, false),
            '2' => (10, true),
            'u' => (11, false),
            '?' => (2, false),
            _ => continue,
        };
        let parts: Vec<&str> = rec.splitn(fields, ' ').collect();
        if parts.len() != fields {
            continue;
        }
        // The original path of a rename is the record after this one.
        let orig_path = if has_orig {
            records.next().map(str::to_owned)
        } else {
            None
        };
        let (x, y) = if kind == '?' {
            ('?', '?')
        } else {
            let mut xy = parts[1].chars();
            (xy.next().unwrap_or('.'), xy.next().unwrap_or('.'))
        };
        status.files.push(FileStatus {
            path: parts[fields - 1].to_owned(),
            orig_path,
            index: x,
            worktree: y,
            badge: badge(kind, x, y),
        });
    }
    status
}

/// Whether a line of a diff is context, added or removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// A line of a hunk, with its number in the old and the new file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

/// A hunk: the `@@ … @@` header and its lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// The difference of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    /// A binary file has no hunks to show.
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

/// The start lines of `@@ -a,b +c,d @@`.
fn hunk_starts(header: &str) -> Option<(u32, u32)> {
    let mut it = header.strip_prefix("@@ -")?.split(' ');
    let old = it.next()?.split(',').next()?.parse().ok()?;
    let new = it
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some((old, new))
}

/// Parses the unified diff of one file.
#[must_use]
pub fn parse_diff(out: &str) -> FileDiff {
    let mut diff = FileDiff::default();
    let (mut old_no, mut new_no) = (0, 0);
    for line in out.lines() {
        if let Some((o, n)) = line.starts_with("@@ ").then(|| hunk_starts(line)).flatten() {
            (old_no, new_no) = (o, n);
            diff.hunks.push(Hunk {
                header: line.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = diff.hunks.last_mut() else {
            if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                diff.binary = true;
            }
            continue;
        };
        // A line of a hunk always starts with a marker, so a bare `diff` is the next file.
        let (kind, text) = match line.as_bytes().first() {
            Some(b'+') => (LineKind::Added, &line[1..]),
            Some(b'-') => (LineKind::Removed, &line[1..]),
            Some(b' ') => (LineKind::Context, &line[1..]),
            Some(b'\\') => continue,
            None => (LineKind::Context, ""),
            Some(_) => break,
        };
        let (o, n) = match kind {
            LineKind::Context => (Some(old_no), Some(new_no)),
            LineKind::Added => (None, Some(new_no)),
            LineKind::Removed => (Some(old_no), None),
        };
        old_no += u32::from(o.is_some());
        new_no += u32::from(n.is_some());
        hunk.lines.push(DiffLine {
            kind,
            old_no: o,
            new_no: n,
            text: text.to_owned(),
        });
    }
    diff
}

/// The diff of a file that Git does not know yet: every line added.
#[must_use]
pub fn added_file_diff(text: &str) -> FileDiff {
    let lines: Vec<DiffLine> = text
        .lines()
        .zip(1..)
        .map(|(l, n)| DiffLine {
            kind: LineKind::Added,
            old_no: None,
            new_no: Some(n),
            text: l.to_owned(),
        })
        .collect();
    if lines.is_empty() {
        return FileDiff::default();
    }
    FileDiff {
        binary: false,
        hunks: vec![Hunk {
            header: format!("@@ -0,0 +1,{} @@", lines.len()),
            lines,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_branch_and_files() {
        let out = "# branch.oid 1234abcd\0# branch.head main\0# branch.upstream origin/main\0\
                   # branch.ab +2 -1\0\
                   1 .M N... 100644 100644 100644 aaa bbb a.sql\0\
                   1 A. N... 000000 100644 100644 000 ccc dir/new one.sql\0\
                   1 .D N... 100644 100644 000000 aaa aaa gone.sql\0\
                   2 R. N... 100644 100644 100644 aaa aaa R100 renamed.sql\0old.sql\0\
                   u UU N... 100644 100644 100644 100644 a b c conflict.sql\0\
                   ? loose.sql\0! ignored.tmp\0";
        let s = parse_status(out);
        assert_eq!(
            s.branch,
            Branch {
                name: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 1,
                has_commits: true,
            }
        );
        let got: Vec<(&str, char)> = s.files.iter().map(|f| (f.path.as_str(), f.badge)).collect();
        assert_eq!(
            got,
            [
                ("a.sql", 'M'),
                ("dir/new one.sql", 'A'),
                ("gone.sql", 'D'),
                ("renamed.sql", 'R'),
                ("conflict.sql", 'U'),
                ("loose.sql", '?'),
            ]
        );
        assert_eq!(s.files[3].orig_path.as_deref(), Some("old.sql"));
        assert_eq!((s.files[0].index, s.files[0].worktree), ('.', 'M'));
    }

    #[test]
    fn status_of_a_new_repository_and_a_detached_head() {
        let s = parse_status("# branch.oid (initial)\0# branch.head main\0? a.sql\0");
        assert!(!s.branch.has_commits);
        assert_eq!(s.branch.upstream, None);
        assert_eq!((s.branch.ahead, s.branch.behind), (0, 0));
        assert_eq!(s.files.len(), 1);
        let s = parse_status("# branch.oid abc\0# branch.head (detached)\0");
        assert_eq!(s.branch.name, None);
        assert_eq!(s.files.len(), 0);
    }

    #[test]
    fn status_skips_what_it_does_not_know() {
        let s = parse_status("garbage\0\x001 short\0x what\0? ok.sql\0");
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].path, "ok.sql");
    }

    #[test]
    fn diff_numbers_the_lines() {
        let out = "diff --git a/a.sql b/a.sql\nindex 1..2 100644\n--- a/a.sql\n+++ b/a.sql\n\
                   @@ -1,3 +1,3 @@\n select 1;\n-select 2;\n+select 22;\n -- keep\n\
                   @@ -10 +10,2 @@ ctx\n-old\n+new\n+more\n\\ No newline at end of file\n";
        let d = parse_diff(out);
        assert!(!d.binary);
        assert_eq!(d.hunks.len(), 2);
        let l = &d.hunks[0].lines;
        assert_eq!(l.len(), 4);
        assert_eq!(
            (l[1].kind, l[1].old_no, l[1].new_no),
            (LineKind::Removed, Some(2), None)
        );
        assert_eq!(
            (l[2].kind, l[2].old_no, l[2].new_no),
            (LineKind::Added, None, Some(2))
        );
        assert_eq!((l[3].old_no, l[3].new_no), (Some(3), Some(3)));
        assert_eq!(l[3].text, "-- keep");
        let l = &d.hunks[1].lines;
        assert_eq!(l.len(), 3);
        assert_eq!(l[2].new_no, Some(11));
    }

    #[test]
    fn diff_of_a_binary_or_empty_output() {
        assert!(parse_diff("diff --git a/x b/x\nBinary files a/x and b/x differ\n").binary);
        assert_eq!(parse_diff(""), FileDiff::default());
    }

    #[test]
    fn a_new_file_is_all_added() {
        let d = added_file_diff("a\nb\n");
        assert_eq!(d.hunks[0].header, "@@ -0,0 +1,2 @@");
        assert_eq!(d.hunks[0].lines[1].new_no, Some(2));
        assert_eq!(added_file_diff(""), FileDiff::default());
    }
}
