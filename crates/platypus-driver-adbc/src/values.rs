//! Values some drivers hand over in a form of their own, turned into the app's: UUIDs as
//! 16 bytes (`arrow.uuid`), and Trino's maps and rows as Go's debug text (`map[k:1]`,
//! `{[a b] [1 x] true}`), shown as the app shows other maps and structs (`{k: 1}`,
//! `{a: 1, b: x}`).

use std::sync::Arc;

use arrow_array::cast::AsArray as _;
use arrow_array::{Array as _, ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use platypus_core::{META_DB_TYPE, Result};

const EXTENSION: &str = "ARROW:extension:name";
/// The driver's name of the column's type.
const DATABASE_TYPE: &str = "sql.database_type_name";

/// How a column's values are turned into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rewrite {
    Uuid,
    /// Go's text of a map, a row or an array of them.
    GoText,
}

fn rewrite(field: &Field) -> Option<Rewrite> {
    let extension = field.metadata().get(EXTENSION).map(String::as_str);
    match (extension, field.data_type()) {
        (Some("arrow.uuid"), DataType::FixedSizeBinary(16)) => Some(Rewrite::Uuid),
        (Some("arrow.opaque"), DataType::Utf8) => {
            let name = field.metadata().get(DATABASE_TYPE)?.to_ascii_uppercase();
            ["MAP(", "ROW(", "ARRAY("]
                .iter()
                .any(|p| name.starts_with(p))
                .then_some(Rewrite::GoText)
        }
        _ => None,
    }
}

/// `schema` with the columns [`batch`] rewrites as text, their database type kept.
#[must_use]
pub fn schema(schema: &SchemaRef) -> SchemaRef {
    if !schema.fields().iter().any(|f| rewrite(f).is_some()) {
        return Arc::clone(schema);
    }
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .map(|f| {
            let Some(_) = rewrite(f) else {
                return f.as_ref().clone();
            };
            let mut metadata = f.metadata().clone();
            if let Some(name) = metadata.get(DATABASE_TYPE).cloned() {
                metadata.insert(META_DB_TYPE.to_owned(), name.to_ascii_lowercase());
            }
            metadata.remove(EXTENSION);
            metadata.remove("ARROW:extension:metadata");
            Field::new(f.name(), DataType::Utf8, true).with_metadata(metadata)
        })
        .collect();
    Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

/// `batch` with its UUIDs and Go texts rewritten, as `target` (its [`schema`]) says.
///
/// # Errors
/// When the batch does not fit `target`.
pub fn batch(batch: RecordBatch, target: &SchemaRef) -> Result<RecordBatch> {
    let source = batch.schema();
    if Arc::ptr_eq(&source, target) || source.as_ref() == target.as_ref() {
        return Ok(batch);
    }
    let columns: Vec<ArrayRef> = source
        .fields()
        .iter()
        .zip(batch.columns())
        .map(|(field, column)| match rewrite(field) {
            Some(Rewrite::Uuid) => {
                let bytes = column.as_fixed_size_binary();
                let text: StringArray = (0..bytes.len())
                    .map(|i| bytes.is_valid(i).then(|| uuid_text(bytes.value(i))))
                    .collect();
                Arc::new(text) as ArrayRef
            }
            Some(Rewrite::GoText) => {
                let text: StringArray = column
                    .as_string::<i32>()
                    .iter()
                    .map(|v| v.map(go_value))
                    .collect();
                Arc::new(text) as ArrayRef
            }
            None => Arc::clone(column),
        })
        .collect();
    Ok(RecordBatch::try_new(Arc::clone(target), columns)?)
}

/// 16 bytes as `8-4-4-4-12` hex digits.
fn uuid_text(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    if hex.len() != 32 {
        return hex;
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The parts of `text` separated by `sep` outside brackets and braces.
fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// A value of Go's `%v` text as the app writes nested values; the text itself when it does
/// not read as one (a text with spaces in it cannot be told from two values).
fn go_value(text: &str) -> String {
    try_go_value(text).unwrap_or_else(|| text.to_owned())
}

fn try_go_value(text: &str) -> Option<String> {
    if text == "<nil>" {
        return Some("NULL".to_owned());
    }
    if let Some(body) = text.strip_prefix("map[").and_then(|b| b.strip_suffix(']')) {
        if body.is_empty() {
            return Some("{}".to_owned());
        }
        let entries = split_top(body, ' ')
            .into_iter()
            .map(|entry| {
                let parts = split_top(entry, ':');
                let [key, value] = parts.as_slice() else {
                    return None;
                };
                Some(format!("{}: {}", try_go_value(key)?, try_go_value(value)?))
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(format!("{{{}}}", entries.join(", ")));
    }
    if let Some(body) = text.strip_prefix('{').and_then(|b| b.strip_suffix('}')) {
        // A row: `{[names] [values] valid}`.
        let parts = split_top(body, ' ');
        let [names, values, _valid] = parts.as_slice() else {
            return None;
        };
        let names = split_top(names.strip_prefix('[')?.strip_suffix(']')?, ' ');
        let values = split_top(values.strip_prefix('[')?.strip_suffix(']')?, ' ');
        if names.len() != values.len() {
            return None;
        }
        let fields = names
            .iter()
            .zip(values)
            .map(|(name, value)| Some(format!("{name}: {}", try_go_value(value)?)))
            .collect::<Option<Vec<_>>>()?;
        return Some(format!("{{{}}}", fields.join(", ")));
    }
    if let Some(body) = text.strip_prefix('[').and_then(|b| b.strip_suffix(']')) {
        if body.is_empty() {
            return Some("[]".to_owned());
        }
        let items = split_top(body, ' ')
            .into_iter()
            .map(try_go_value)
            .collect::<Option<Vec<_>>>()?;
        return Some(format!("[{}]", items.join(", ")));
    }
    Some(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_go_text() {
        assert_eq!(go_value("map[k:1]"), "{k: 1}");
        assert_eq!(go_value("{[a b] [1 x] true}"), "{a: 1, b: x}");
        assert_eq!(
            go_value("map[j:<nil> k:{[field0] [1] true}]"),
            "{j: NULL, k: {field0: 1}}"
        );
        assert_eq!(
            go_value("[{[field0 field1] [1 y] true}]"),
            "[{field0: 1, field1: y}]"
        );
        // A text with a space in it: two values or one? Left as it came.
        assert_eq!(go_value("{[a] [x y] true}"), "{[a] [x y] true}");
    }

    #[test]
    fn writes_uuids_with_dashes() {
        let bytes = [
            0xb0, 0xee, 0xbc, 0x99, 0x9c, 0x0b, 0x4e, 0xf8, 0xbb, 0x6d, 0x6b, 0xb9, 0xbd, 0x38,
            0x0a, 0x12,
        ];
        assert_eq!(uuid_text(&bytes), "b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12");
    }
}
