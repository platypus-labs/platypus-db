//! Values as the JSON export writes them: numbers and booleans bare, values that are JSON
//! already (`json` and `jsonb` columns, MongoDB arrays and documents, Arrow lists and
//! structs) and PostgreSQL arrays as JSON, the rest as strings.

use std::iter::Peekable;
use std::str::Chars;

use arrow_schema::{DataType, Field};
use platypus_core::META_DB_TYPE;
use platypus_sql::dml::ValueKind;

/// How a column's values are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonKind {
    /// By their [`ValueKind`]: numbers and booleans bare, the rest as strings.
    Plain,
    /// JSON text, written as it is (a string when it is not JSON after all).
    Json,
    /// A PostgreSQL array's text, of elements of that kind.
    PgArray(Element),
}

/// The kind of a PostgreSQL array's elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    Number,
    Bool,
    Text,
}

impl Element {
    fn of(declared: &str) -> Self {
        let base = declared.split('(').next().unwrap_or_default().trim();
        match base {
            "int2" | "int4" | "int8" | "smallint" | "integer" | "bigint" | "float4" | "float8"
            | "real" | "double precision" | "numeric" | "decimal" | "oid" => Self::Number,
            "bool" | "boolean" => Self::Bool,
            _ => Self::Text,
        }
    }
}

impl JsonKind {
    /// The kind of the values of `field`, from its type and the type the database declares.
    #[must_use]
    pub fn of(field: &Field) -> Self {
        match field.data_type() {
            DataType::List(_)
            | DataType::LargeList(_)
            | DataType::FixedSizeList(..)
            | DataType::Struct(_)
            | DataType::Map(..) => return Self::Json,
            DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {}
            _ => return Self::Plain,
        }
        let Some(declared) = field.metadata().get(META_DB_TYPE) else {
            return Self::Plain;
        };
        let declared = declared.trim().to_ascii_lowercase();
        if matches!(declared.as_str(), "json" | "jsonb" | "array" | "document") {
            return Self::Json;
        }
        declared
            .strip_suffix("[]")
            .map_or(Self::Plain, |element| Self::PgArray(Element::of(element)))
    }

    /// Writes `value` (`None` for NULL) of a column of `kind` to `s`.
    pub fn value(self, s: &mut String, kind: ValueKind, value: Option<&str>) {
        let Some(v) = value else {
            s.push_str("null");
            return;
        };
        match self {
            Self::Json if is_json(v) => s.push_str(v),
            Self::PgArray(element) => match pg_array(v, element) {
                Some(json) => s.push_str(&json),
                None => string(s, v),
            },
            _ => match kind {
                ValueKind::Integer | ValueKind::Float | ValueKind::Real if is_number(v) => {
                    s.push_str(v);
                }
                ValueKind::Bool if v == "true" || v == "false" => s.push_str(v),
                _ => string(s, v),
            },
        }
    }
}

/// `text` as a JSON string.
pub fn string(s: &mut String, text: &str) {
    s.push_str(&serde_json::Value::from(text).to_string());
}

fn is_json(text: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok()
}

/// Whether `text` is a JSON number, one a double holds (not NaN, an infinity or `1e400`).
fn is_number(text: &str) -> bool {
    serde_json::from_str::<serde_json::Number>(text).is_ok()
}

/// A PostgreSQL array's text (`{1,2}`, `{{a,"b c"},{NULL,d}}`) as a JSON array; `None` for
/// text it does not read, e.g. an array with bounds (`[0:1]={1,2}`).
fn pg_array(text: &str, element: Element) -> Option<String> {
    let mut chars = text.chars().peekable();
    let mut out = String::with_capacity(text.len() + 2);
    pg_elements(&mut chars, element, &mut out)?;
    chars.next().is_none().then_some(out)
}

fn pg_elements(chars: &mut Peekable<Chars<'_>>, element: Element, out: &mut String) -> Option<()> {
    if chars.next()? != '{' {
        return None;
    }
    out.push('[');
    if chars.peek() == Some(&'}') {
        chars.next();
        out.push(']');
        return Some(());
    }
    loop {
        match *chars.peek()? {
            '{' => pg_elements(chars, element, out)?,
            '"' => {
                chars.next();
                let mut v = String::new();
                loop {
                    match chars.next()? {
                        '\\' => v.push(chars.next()?),
                        '"' => break,
                        c => v.push(c),
                    }
                }
                pg_element(out, &v, element);
            }
            _ => {
                let mut v = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ',' || c == '}' {
                        break;
                    }
                    v.push(c);
                    chars.next();
                }
                let v = v.trim();
                if v.eq_ignore_ascii_case("NULL") {
                    out.push_str("null");
                } else {
                    pg_element(out, v, element);
                }
            }
        }
        match chars.next()? {
            ',' => out.push(','),
            '}' => {
                out.push(']');
                return Some(());
            }
            _ => return None,
        }
    }
}

fn pg_element(out: &mut String, v: &str, element: Element) {
    match element {
        Element::Number if is_number(v) => out.push_str(v),
        Element::Bool if v == "t" || v == "true" => out.push_str("true"),
        Element::Bool if v == "f" || v == "false" => out.push_str("false"),
        _ => string(out, v),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn written(kind: JsonKind, value_kind: ValueKind, value: Option<&str>) -> String {
        let mut s = String::new();
        kind.value(&mut s, value_kind, value);
        s
    }

    #[test]
    fn kinds_of_columns() {
        let declared = |t: &str| {
            Field::new("c", DataType::Utf8, true)
                .with_metadata(HashMap::from([(META_DB_TYPE.to_owned(), t.to_owned())]))
        };
        assert_eq!(JsonKind::of(&declared("jsonb")), JsonKind::Json);
        assert_eq!(JsonKind::of(&declared("document")), JsonKind::Json);
        assert_eq!(
            JsonKind::of(&declared("int4[]")),
            JsonKind::PgArray(Element::Number)
        );
        assert_eq!(
            JsonKind::of(&declared("character varying(5)[]")),
            JsonKind::PgArray(Element::Text)
        );
        assert_eq!(JsonKind::of(&declared("text")), JsonKind::Plain);
        assert_eq!(
            JsonKind::of(&Field::new("c", DataType::Int64, true)),
            JsonKind::Plain
        );
    }

    #[test]
    fn writes_json_values_as_json() {
        let text = ValueKind::Text;
        assert_eq!(
            written(JsonKind::Json, text, Some(r#"{"a": 1}"#)),
            r#"{"a": 1}"#
        );
        assert_eq!(
            written(JsonKind::Json, text, Some("not json")),
            "\"not json\""
        );
        assert_eq!(written(JsonKind::Json, text, None), "null");
        let numbers = JsonKind::PgArray(Element::Number);
        assert_eq!(written(numbers, text, Some("{1,2}")), "[1,2]");
        assert_eq!(
            written(numbers, text, Some("{{1,NULL},{3,NaN}}")),
            r#"[[1,null],[3,"NaN"]]"#
        );
        let texts = JsonKind::PgArray(Element::Text);
        assert_eq!(
            written(texts, text, Some(r#"{a,"b, c","NULL","q\"x",NULL}"#)),
            r#"["a","b, c","NULL","q\"x",null]"#
        );
        assert_eq!(written(texts, text, Some("{}")), "[]");
        assert_eq!(
            written(texts, text, Some("[0:1]={a,b}")),
            r#""[0:1]={a,b}""#
        );
        let flags = JsonKind::PgArray(Element::Bool);
        assert_eq!(written(flags, text, Some("{t,f}")), "[true,false]");
    }

    #[test]
    fn writes_numbers_bare_when_json_has_them() {
        let plain = JsonKind::Plain;
        assert_eq!(written(plain, ValueKind::Integer, Some("42")), "42");
        assert_eq!(written(plain, ValueKind::Float, Some("1.5")), "1.5");
        assert_eq!(written(plain, ValueKind::Float, Some("NaN")), "\"NaN\"");
        assert_eq!(written(plain, ValueKind::Decimal, Some("1.50")), "\"1.50\"");
        assert_eq!(written(plain, ValueKind::Bool, Some("true")), "true");
        assert_eq!(written(plain, ValueKind::Text, Some("a\"b")), "\"a\\\"b\"");
    }
}
