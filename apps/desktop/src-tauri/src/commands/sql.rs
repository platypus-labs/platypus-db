//! SQL text analysis for the editor.

use platypus_core::DriverKind;
use platypus_sql::{Dialect, FormatOptions, KeywordCase, SelectItem};
use serde::{Deserialize, Serialize};

/// A statement range in UTF-16 code units, the unit JavaScript strings use.
#[derive(Debug, Serialize)]
pub struct StatementSpan {
    start: usize,
    end: usize,
    read_only: bool,
}

#[derive(Debug, Serialize)]
pub struct SqlAnalysis {
    statements: Vec<StatementSpan>,
    /// Index into `statements` of the one to run for the cursor.
    at_cursor: Option<usize>,
}

/// Maps byte offsets of `src` to UTF-16 offsets and back.
struct Utf16Map {
    /// `(byte offset, utf16 offset)` at every char boundary, plus the end.
    points: Vec<(usize, usize)>,
}

impl Utf16Map {
    fn new(src: &str) -> Self {
        let mut points = Vec::with_capacity(src.len() + 1);
        let mut utf16 = 0;
        for (byte, c) in src.char_indices() {
            points.push((byte, utf16));
            utf16 += c.len_utf16();
        }
        points.push((src.len(), utf16));
        Self { points }
    }

    fn to_utf16(&self, byte: usize) -> usize {
        let i = self.points.partition_point(|&(b, _)| b < byte);
        self.points.get(i).map_or(0, |&(_, u)| u)
    }

    fn to_byte(&self, utf16: usize) -> usize {
        let i = self.points.partition_point(|&(_, u)| u < utf16);
        self.points
            .get(i)
            .map_or_else(|| self.points.last().map_or(0, |p| p.0), |&(b, _)| b)
    }
}

/// Splits `sql` into statements and finds the one under `cursor` (UTF-16 offset).
#[tauri::command]
pub fn analyze_sql(sql: String, driver: DriverKind, cursor: usize) -> SqlAnalysis {
    let dialect = Dialect::from(driver);
    let map = Utf16Map::new(&sql);
    let statements = platypus_sql::split(&sql, dialect);
    let at = platypus_sql::statement_at(&sql, dialect, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
        statements: statements
            .iter()
            .map(|s| StatementSpan {
                start: map.to_utf16(s.start),
                end: map.to_utf16(s.end),
                read_only: platypus_sql::is_read_only(s.text(&sql), dialect),
            })
            .collect(),
    }
}

/// The formatter settings, as the editor sends them.
#[derive(Debug, Deserialize)]
pub struct FormatSettings {
    /// `upper`, `lower` or `preserve`.
    keyword_case: String,
    indent: usize,
}

/// Formats `sql` for the editor; only whitespace and the case of keywords change.
#[tauri::command]
pub fn format_sql(sql: String, driver: DriverKind, settings: FormatSettings) -> String {
    let keyword_case = match settings.keyword_case.as_str() {
        "lower" => KeywordCase::Lower,
        "preserve" => KeywordCase::Preserve,
        _ => KeywordCase::Upper,
    };
    platypus_sql::format(
        &sql,
        Dialect::from(driver),
        FormatOptions {
            keyword_case,
            indent: settings.indent.clamp(1, 8),
        },
    )
}

/// An item of a query's select list, see [`SelectItem`].
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceItem {
    All,
    Column { name: String },
    Other,
}

/// The one table a query reads, so its results can be edited.
#[derive(Debug, Serialize)]
pub struct QuerySource {
    schema: Option<String>,
    name: String,
    items: Vec<SourceItem>,
}

/// The table `sql` reads when its rows are that table's rows; `None` for joins, grouping
/// and anything but a plain `SELECT`.
#[tauri::command]
pub fn query_source(sql: String, driver: DriverKind) -> Option<QuerySource> {
    let source = platypus_sql::source_table(&sql, Dialect::from(driver))?;
    Some(QuerySource {
        schema: source.schema,
        name: source.name,
        items: source
            .items
            .into_iter()
            .map(|item| match item {
                SelectItem::All => SourceItem::All,
                SelectItem::Column(name) => SourceItem::Column { name },
                SelectItem::Other => SourceItem::Other,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_utf16_offsets() {
        // "é" is 2 bytes / 1 unit, "🦆" is 4 bytes / 2 units.
        let map = Utf16Map::new("aé🦆b");
        assert_eq!(map.to_utf16(0), 0);
        assert_eq!(map.to_utf16(3), 2);
        assert_eq!(map.to_utf16(7), 4);
        assert_eq!(map.to_utf16(8), 5);
        assert_eq!(map.to_byte(4), 7);
        assert_eq!(map.to_byte(99), 8);
    }

    #[test]
    fn analyzes_in_utf16_units() {
        let sql = "select '🦆';\nupdate t set x = 1".to_owned();
        let a = analyze_sql(sql, DriverKind::Postgres, 14);
        assert_eq!(a.statements.len(), 2);
        assert_eq!((a.statements[0].start, a.statements[0].end), (0, 11));
        assert!(a.statements[0].read_only);
        assert_eq!(a.statements[1].start, 13);
        assert!(!a.statements[1].read_only);
        assert_eq!(a.at_cursor, Some(1));
    }
}
