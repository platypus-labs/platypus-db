//! Values as mongosh reads them: relaxed JSON (unquoted keys, single quotes, trailing
//! commas, comments), the shell helpers (`ObjectId("…")`, `ISODate("…")`, `NumberLong(…)`,
//! …), regular expression literals and Extended JSON (`{"$oid": "…"}`).

use std::str::FromStr;

use mongodb::bson::spec::BinarySubtype;
use mongodb::bson::{
    Binary, Bson, DateTime, Decimal128, Document, Regex, Timestamp, oid::ObjectId, uuid::Uuid,
};

/// A syntax error at a byte offset of the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
}

pub type ParseResult<T> = Result<T, ParseError>;

/// Extended JSON keys of a document that stands for a single BSON value.
const EXTENDED: &[&str] = &[
    "$oid",
    "$date",
    "$numberLong",
    "$numberInt",
    "$numberDouble",
    "$numberDecimal",
    "$binary",
    "$uuid",
    "$timestamp",
    "$regularExpression",
    "$minKey",
    "$maxKey",
    "$undefined",
    "$symbol",
    "$code",
];

/// A cursor over the text of one statement.
#[derive(Debug)]
pub struct Reader<'a> {
    pub src: &'a str,
    pub pos: usize,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    pub fn error<T>(&self, message: impl Into<String>) -> ParseResult<T> {
        Err(ParseError {
            message: message.into(),
            offset: self.pos,
        })
    }

    #[must_use]
    pub fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    #[must_use]
    pub fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub fn at_end(&mut self) -> bool {
        self.skip_space();
        self.pos >= self.src.len()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Skips whitespace and `//` and `/* */` comments.
    pub fn skip_space(&mut self) {
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.pos += rest.len() - trimmed.len();
            if trimmed.starts_with("//") {
                self.pos += trimmed.find('\n').unwrap_or(trimmed.len());
            } else if let Some(body) = trimmed.strip_prefix("/*") {
                self.pos += body.find("*/").map_or(trimmed.len(), |i| i + 4);
            } else {
                return;
            }
        }
    }

    /// Consumes `c` after optional space.
    pub fn eat(&mut self, c: char) -> bool {
        self.skip_space();
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, c: char) -> ParseResult<()> {
        if self.eat(c) {
            Ok(())
        } else {
            self.error(format!("expected `{c}`"))
        }
    }

    /// A JavaScript identifier (letters, digits, `_` and `$`), after optional space.
    pub fn ident(&mut self) -> Option<&'a str> {
        self.skip_space();
        let rest = self.rest();
        let len = rest
            .char_indices()
            .find(|&(i, c)| {
                !(c == '_' || c == '$' || c.is_alphabetic() || (i > 0 && c.is_ascii_digit()))
            })
            .map_or(rest.len(), |(i, _)| i);
        if len == 0 {
            return None;
        }
        self.pos += len;
        Some(&rest[..len])
    }

    /// A single- or double-quoted string with JavaScript escapes.
    pub fn string(&mut self) -> ParseResult<String> {
        self.skip_space();
        let start = self.pos;
        let Some(quote @ ('"' | '\'' | '`')) = self.bump() else {
            return self.error("expected a string");
        };
        let mut out = String::new();
        loop {
            match self.bump() {
                None => {
                    self.pos = start;
                    return self.error("unterminated string");
                }
                Some(c) if c == quote => return Ok(out),
                Some('\\') => out.push(self.escape()?),
                Some(c) => out.push(c),
            }
        }
    }

    fn escape(&mut self) -> ParseResult<char> {
        Ok(match self.bump() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some('r') => '\r',
            Some('b') => '\u{8}',
            Some('f') => '\u{c}',
            Some('v') => '\u{b}',
            Some('0') => '\0',
            Some('u') => {
                let braced = self.peek() == Some('{');
                let hex: String = if braced {
                    self.pos += 1;
                    let end = self.rest().find('}').unwrap_or(0);
                    let hex = self.rest()[..end].to_owned();
                    self.pos += end + 1;
                    hex
                } else {
                    let hex = self.rest().get(..4).unwrap_or("").to_owned();
                    self.pos += hex.len();
                    hex
                };
                let code = u32::from_str_radix(&hex, 16).ok();
                match code.and_then(char::from_u32) {
                    Some(c) => c,
                    None => return self.error("invalid \\u escape"),
                }
            }
            Some(c) => c,
            None => return self.error("unterminated string"),
        })
    }

    /// Any value: a document, array, string, number, regex, keyword or shell helper.
    pub fn value(&mut self) -> ParseResult<Bson> {
        self.skip_space();
        match self.peek() {
            Some('{') => self.document().map(Bson::Document),
            Some('[') => self.array().map(Bson::Array),
            Some('"' | '\'' | '`') => self.string().map(Bson::String),
            Some('/') => self.regex(),
            Some(c) if c.is_ascii_digit() || matches!(c, '-' | '+' | '.') => self.number(),
            Some(_) => self.word(),
            None => self.error("expected a value"),
        }
    }

    /// A `{ … }` document; one written in Extended JSON becomes the value it stands for
    /// only through [`Self::value`].
    pub fn document(&mut self) -> ParseResult<Document> {
        self.expect('{')?;
        let mut doc = Document::new();
        while !self.eat('}') {
            self.skip_space();
            let key = match self.peek() {
                Some('"' | '\'' | '`') => self.string()?,
                Some(c) if c.is_ascii_digit() => {
                    let start = self.pos;
                    self.number()?;
                    self.src[start..self.pos].to_owned()
                }
                _ => match self.ident() {
                    Some(key) => key.to_owned(),
                    None => return self.error("expected a field name"),
                },
            };
            self.expect(':')?;
            let value = self.value()?;
            doc.insert(key, value);
            if !self.eat(',') {
                self.expect('}')?;
                break;
            }
        }
        Ok(doc)
    }

    fn array(&mut self) -> ParseResult<Vec<Bson>> {
        self.expect('[')?;
        let mut items = Vec::new();
        while !self.eat(']') {
            items.push(self.value()?);
            if !self.eat(',') {
                self.expect(']')?;
                break;
            }
        }
        Ok(items)
    }

    fn regex(&mut self) -> ParseResult<Bson> {
        let start = self.pos;
        self.pos += 1;
        let mut pattern = String::new();
        let mut class = false;
        loop {
            match self.bump() {
                None | Some('\n') => {
                    self.pos = start;
                    return self.error("unterminated regular expression");
                }
                Some('/') if !class => break,
                Some('\\') => {
                    pattern.push('\\');
                    if let Some(c) = self.bump() {
                        pattern.push(c);
                    }
                }
                Some(c) => {
                    class = match c {
                        '[' => true,
                        ']' => false,
                        _ => class,
                    };
                    pattern.push(c);
                }
            }
        }
        let flags_len = self
            .rest()
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(self.rest().len());
        let flags = self.rest()[..flags_len].to_owned();
        self.pos += flags_len;
        Ok(Bson::RegularExpression(regex(pattern, flags)))
    }

    fn number(&mut self) -> ParseResult<Bson> {
        let start = self.pos;
        if matches!(self.peek(), Some('-' | '+')) {
            self.pos += 1;
        }
        if let Some(word) = self.rest().strip_prefix("Infinity") {
            let _ = word;
            self.pos += "Infinity".len();
            let negative = self.src[start..].starts_with('-');
            return Ok(Bson::Double(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }));
        }
        let len = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
            .unwrap_or(self.rest().len());
        let mut end = self.pos + len;
        // `1e-5`: the sign of an exponent.
        while self.src[..end].ends_with(['e', 'E'])
            && self.src[end..].starts_with(['-', '+'])
            && !self.src[start..].starts_with("0x")
        {
            let more = self.src[end + 1..]
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(self.src.len() - end - 1);
            end += 1 + more;
        }
        self.pos = end;
        let text: String = self.src[start..end]
            .chars()
            .filter(|&c| c != '_' && c != '+')
            .collect();
        parse_number(&text).map_or_else(
            || {
                self.pos = start;
                self.error(format!("invalid number `{text}`"))
            },
            Ok,
        )
    }

    /// A keyword (`true`, `null`, …) or a shell helper call (`ObjectId("…")`).
    fn word(&mut self) -> ParseResult<Bson> {
        let start = self.pos;
        let Some(mut name) = self.ident() else {
            return self.error("expected a value");
        };
        match name {
            "true" => return Ok(Bson::Boolean(true)),
            "false" => return Ok(Bson::Boolean(false)),
            "null" => return Ok(Bson::Null),
            "undefined" => return Ok(Bson::Undefined),
            "NaN" => return Ok(Bson::Double(f64::NAN)),
            "Infinity" => return Ok(Bson::Double(f64::INFINITY)),
            "MinKey" | "MaxKey" if !self.rest().trim_start().starts_with('(') => {
                return Ok(if name == "MinKey" {
                    Bson::MinKey
                } else {
                    Bson::MaxKey
                });
            }
            "new" => match self.ident() {
                Some(next) => name = next,
                None => return self.error("expected a constructor after `new`"),
            },
            _ => {}
        }
        if !self.eat('(') {
            self.pos = start;
            return self.error(format!("unknown value `{name}`"));
        }
        let mut args = Vec::new();
        while !self.eat(')') {
            args.push(self.value()?);
            if !self.eat(',') {
                self.expect(')')?;
                break;
            }
        }
        helper(name, &args).map_err(|message| ParseError {
            message,
            offset: start,
        })
    }
}

/// A whole number of any BSON numeric type.
#[must_use]
pub fn integer(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(n) => Some(i64::from(*n)),
        Bson::Int64(n) => Some(*n),
        #[allow(clippy::cast_possible_truncation)]
        Bson::Double(d) if d.fract() == 0.0 => Some(*d as i64),
        _ => None,
    }
}

/// A regular expression with its flags sorted, as BSON keeps them.
fn regex(pattern: impl Into<String>, flags: impl Into<String>) -> Regex {
    let mut options: Vec<char> = flags.into().chars().collect();
    options.sort_unstable();
    Regex {
        pattern: pattern.into(),
        options: options.into_iter().collect(),
    }
}

fn parse_number(text: &str) -> Option<Bson> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        let value = i64::from_str_radix(hex, 16).ok()?;
        return Some(int(if negative { -value } else { value }));
    }
    if let Ok(value) = text.parse::<i64>() {
        return Some(int(value));
    }
    text.parse::<f64>().ok().map(Bson::Double)
}

/// An integer as mongosh stores it: a 32-bit int when it fits, a double otherwise (as
/// JavaScript numbers are); `NumberLong` is the way to a 64-bit int.
fn int(value: i64) -> Bson {
    i32::try_from(value).map_or_else(
        |_| {
            #[allow(clippy::cast_precision_loss)]
            Bson::Double(value as f64)
        },
        Bson::Int32,
    )
}

fn arg_text(args: &[Bson], i: usize) -> Option<String> {
    match args.get(i)? {
        Bson::String(s) => Some(s.clone()),
        Bson::Int32(n) => Some(n.to_string()),
        Bson::Int64(n) => Some(n.to_string()),
        Bson::Double(n) => Some(n.to_string()),
        _ => None,
    }
}

fn arg_u32(args: &[Bson], i: usize) -> Result<u32, String> {
    match args.get(i) {
        Some(Bson::Int32(n)) => u32::try_from(*n).map_err(|e| e.to_string()),
        Some(Bson::Int64(n)) => u32::try_from(*n).map_err(|e| e.to_string()),
        _ => Err("expected a number".to_owned()),
    }
}

/// The value of a shell helper such as `ObjectId("…")` or `new Date()`.
fn helper(name: &str, args: &[Bson]) -> Result<Bson, String> {
    let text = arg_text(args, 0);
    match name {
        "ObjectId" => match text {
            None => Ok(Bson::ObjectId(ObjectId::new())),
            Some(hex) => ObjectId::parse_str(&hex)
                .map(Bson::ObjectId)
                .map_err(|e| format!("invalid ObjectId: {e}")),
        },
        "ISODate" | "Date" => date(args.first()).map(Bson::DateTime),
        "NumberLong" | "Long" => text
            .and_then(|t| t.parse::<i64>().ok())
            .map(Bson::Int64)
            .ok_or_else(|| format!("{name} expects an integer")),
        "NumberInt" | "Int32" => text
            .and_then(|t| t.parse::<i32>().ok())
            .map(Bson::Int32)
            .ok_or_else(|| format!("{name} expects a 32-bit integer")),
        "Double" => text
            .and_then(|t| t.parse::<f64>().ok())
            .map(Bson::Double)
            .ok_or_else(|| "Double expects a number".to_owned()),
        "NumberDecimal" | "Decimal128" => text
            .and_then(|t| Decimal128::from_str(&t).ok())
            .map(Bson::Decimal128)
            .ok_or_else(|| format!("{name} expects a decimal number")),
        "UUID" => match text {
            None => Ok(Bson::Binary(Uuid::new().into())),
            Some(t) => Uuid::parse_str(&t)
                .map(|u| Bson::Binary(u.into()))
                .map_err(|e| format!("invalid UUID: {e}")),
        },
        "BinData" => {
            let subtype = u8::try_from(arg_u32(args, 0)?).map_err(|e| e.to_string())?;
            let data = arg_text(args, 1).ok_or("BinData expects base64 data")?;
            Binary::from_base64(&data, BinarySubtype::from(subtype))
                .map(Bson::Binary)
                .map_err(|e| format!("invalid BinData: {e}"))
        }
        "Timestamp" => {
            let (time, increment) = match args.first() {
                Some(Bson::Document(d)) => (
                    d.get("t").and_then(integer).unwrap_or(0),
                    d.get("i").and_then(integer).unwrap_or(0),
                ),
                _ if args.is_empty() => (0, 0),
                _ => (i64::from(arg_u32(args, 0)?), i64::from(arg_u32(args, 1)?)),
            };
            Ok(Bson::Timestamp(Timestamp {
                time: u32::try_from(time).map_err(|e| e.to_string())?,
                increment: u32::try_from(increment).map_err(|e| e.to_string())?,
            }))
        }
        "RegExp" => Ok(Bson::RegularExpression(regex(
            text.ok_or("RegExp expects a pattern")?,
            arg_text(args, 1).unwrap_or_default(),
        ))),
        "MinKey" => Ok(Bson::MinKey),
        "MaxKey" => Ok(Bson::MaxKey),
        _ => Err(format!("unknown function `{name}`")),
    }
}

fn date(arg: Option<&Bson>) -> Result<DateTime, String> {
    match arg {
        None => Ok(DateTime::now()),
        Some(Bson::Int32(ms)) => Ok(DateTime::from_millis(i64::from(*ms))),
        Some(Bson::Int64(ms)) => Ok(DateTime::from_millis(*ms)),
        #[allow(clippy::cast_possible_truncation)]
        Some(Bson::Double(ms)) => Ok(DateTime::from_millis(*ms as i64)),
        Some(Bson::String(s)) => parse_date(s).ok_or_else(|| format!("invalid date `{s}`")),
        Some(_) => Err("a date expects a string or milliseconds".to_owned()),
    }
}

/// An ISO 8601 date as `ISODate` takes it: a date alone, or a time without seconds or zone.
fn parse_date(s: &str) -> Option<DateTime> {
    let s = s.trim().replace(' ', "T");
    let mut candidates = vec![s.clone()];
    if s.len() == 10 {
        candidates.push(format!("{s}T00:00:00Z"));
    }
    if !s.ends_with('Z') && !s[s.len().saturating_sub(6)..].contains(['+', '-']) {
        candidates.push(format!("{s}Z"));
        candidates.push(format!("{s}:00Z"));
    }
    candidates
        .iter()
        .find_map(|c| DateTime::parse_rfc3339_str(c).ok())
}

/// Turns an Extended JSON document (`{"$oid": "…"}`) into the value it stands for,
/// everywhere in `value`.
pub fn resolve_extended(value: Bson) -> Bson {
    match value {
        Bson::Document(doc) => {
            let single_value = doc
                .keys()
                .next()
                .is_some_and(|k| EXTENDED.contains(&k.as_str()));
            if single_value {
                let json = Bson::Document(doc.clone()).into_relaxed_extjson();
                if let Ok(bson) = Bson::try_from(json)
                    && !matches!(bson, Bson::Document(_))
                {
                    return bson;
                }
            }
            Bson::Document(
                doc.into_iter()
                    .map(|(k, v)| (k, resolve_extended(v)))
                    .collect(),
            )
        }
        Bson::Array(items) => Bson::Array(items.into_iter().map(resolve_extended).collect()),
        other => other,
    }
}

/// A value written as mongosh reads it back: `{ a: 1, "b-c": ObjectId("…") }`.
#[must_use]
pub fn to_shell(value: &Bson) -> String {
    let quote = |s: &str| serde_json::Value::String(s.to_owned()).to_string();
    match value {
        Bson::Document(doc) => {
            if doc.is_empty() {
                return "{}".to_owned();
            }
            let fields: Vec<String> = doc
                .iter()
                .map(|(k, v)| {
                    let plain = k
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                        && k.chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '$');
                    let key = if plain { k.clone() } else { quote(k) };
                    format!("{key}: {}", to_shell(v))
                })
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        Bson::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(to_shell).collect::<Vec<_>>().join(", ")
            )
        }
        Bson::String(s) => quote(s),
        Bson::Int32(n) => n.to_string(),
        Bson::Int64(n) => format!("NumberLong({n})"),
        Bson::Double(d) if d.is_nan() => "NaN".to_owned(),
        Bson::Double(d) if d.is_infinite() => {
            if *d > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
        }
        Bson::Double(d) if d.fract() == 0.0 && d.abs() < 1e15 => format!("{d:.1}"),
        Bson::Double(d) => d.to_string(),
        Bson::Boolean(b) => b.to_string(),
        Bson::Null => "null".to_owned(),
        Bson::Undefined => "undefined".to_owned(),
        Bson::ObjectId(id) => format!("ObjectId({})", quote(&id.to_hex())),
        Bson::DateTime(d) => format!(
            "ISODate({})",
            quote(&d.try_to_rfc3339_string().unwrap_or_default())
        ),
        Bson::Decimal128(d) => format!("NumberDecimal({})", quote(&d.to_string())),
        Bson::RegularExpression(r) => format!("/{}/{}", r.pattern, r.options),
        Bson::Timestamp(t) => format!("Timestamp({}, {})", t.time, t.increment),
        Bson::MinKey => "MinKey()".to_owned(),
        Bson::MaxKey => "MaxKey()".to_owned(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use mongodb::bson::doc;

    use super::*;

    fn value(src: &str) -> Bson {
        let mut r = Reader::new(src);
        let v = r.value().unwrap_or_else(|e| panic!("{src}: {e:?}"));
        assert!(r.at_end(), "{src}: trailing text");
        resolve_extended(v)
    }

    fn error(src: &str) -> ParseError {
        Reader::new(src).value().unwrap_err()
    }

    #[test]
    fn reads_relaxed_json() {
        assert_eq!(
            value("{ a: 1, 'b': \"x\", $gt: [1, 2.5, -3,], nested: { ok: true, n: null }, }"),
            Bson::Document(doc! {
                "a": 1, "b": "x", "$gt": [1, 2.5, -3], "nested": { "ok": true, "n": null }
            })
        );
        assert_eq!(
            value("{ /* c */ a: 1 // end\n }"),
            Bson::Document(doc! {"a": 1})
        );
        assert_eq!(value("{ 'a.b': 1 }"), Bson::Document(doc! {"a.b": 1}));
        assert_eq!(value("'it\\'s\\n\\u00e9'"), Bson::String("it's\né".into()));
    }

    #[test]
    fn keeps_field_order() {
        let Bson::Document(d) = value("{ z: 1, a: -1, m: 1 }") else {
            panic!()
        };
        assert_eq!(d.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
    }

    #[test]
    fn reads_numbers() {
        assert_eq!(value("42"), Bson::Int32(42));
        assert_eq!(value("-7"), Bson::Int32(-7));
        assert_eq!(value("1.5e3"), Bson::Double(1500.0));
        assert_eq!(value("1e-2"), Bson::Double(0.01));
        assert_eq!(value("0x1F"), Bson::Int32(31));
        assert_eq!(value("1_000"), Bson::Int32(1000));
        assert_eq!(value("3000000000"), Bson::Double(3e9));
        assert_eq!(value("-Infinity"), Bson::Double(f64::NEG_INFINITY));
        assert_eq!(value("NumberLong(3000000000)"), Bson::Int64(3_000_000_000));
        assert_eq!(value("NumberLong('12')"), Bson::Int64(12));
        assert_eq!(value("NumberInt(5)"), Bson::Int32(5));
        assert_eq!(
            value("NumberDecimal('1.10')"),
            Bson::Decimal128("1.10".parse().unwrap())
        );
    }

    #[test]
    fn reads_shell_helpers() {
        let oid = "65a1b2c3d4e5f60718293a4b";
        assert_eq!(
            value(&format!("ObjectId('{oid}')")),
            Bson::ObjectId(oid.parse().unwrap())
        );
        assert!(matches!(value("new ObjectId()"), Bson::ObjectId(_)));
        let day = DateTime::parse_rfc3339_str("2024-01-02T00:00:00Z").unwrap();
        assert_eq!(value("ISODate('2024-01-02')"), Bson::DateTime(day));
        assert_eq!(
            value("new Date(\"2024-01-02T00:00:00Z\")"),
            Bson::DateTime(day)
        );
        assert_eq!(value("ISODate('2024-01-02T00:00')"), Bson::DateTime(day));
        assert_eq!(
            value("ISODate('2024-01-02T01:00:00+01:00')"),
            Bson::DateTime(day)
        );
        assert!(
            matches!(value("UUID('00112233-4455-6677-8899-aabbccddeeff')"), Bson::Binary(b) if b.subtype == BinarySubtype::Uuid)
        );
        assert!(matches!(value("BinData(0, 'AAE=')"), Bson::Binary(b) if b.bytes == [0, 1]));
        assert_eq!(
            value("Timestamp(5, 1)"),
            Bson::Timestamp(Timestamp {
                time: 5,
                increment: 1
            })
        );
        assert_eq!(value("MinKey()"), Bson::MinKey);
        assert_eq!(value("MaxKey"), Bson::MaxKey);
        assert_eq!(value("undefined"), Bson::Undefined);
    }

    #[test]
    fn reads_regular_expressions() {
        assert_eq!(
            value("/^a[/]b\\/c/i"),
            Bson::RegularExpression(regex("^a[/]b\\/c", "i"))
        );
        assert_eq!(
            value("{ name: /x/ }"),
            Bson::Document(doc! { "name": Bson::RegularExpression(regex("x", "")) })
        );
        assert_eq!(
            value("RegExp('a.c', 'ms')"),
            Bson::RegularExpression(regex("a.c", "ms"))
        );
    }

    #[test]
    fn reads_extended_json() {
        let oid = "65a1b2c3d4e5f60718293a4b";
        assert_eq!(
            value(&format!(
                "{{ _id: {{ \"$oid\": \"{oid}\" }}, n: {{ $numberLong: '9' }} }}"
            )),
            Bson::Document(doc! { "_id": ObjectId::parse_str(oid).unwrap(), "n": 9_i64 })
        );
        assert!(matches!(
            value("{ \"$date\": \"2024-01-02T00:00:00Z\" }"),
            Bson::DateTime(_)
        ));
        // Query operators stay documents.
        assert_eq!(value("{ $gt: 1 }"), Bson::Document(doc! { "$gt": 1 }));
    }

    #[test]
    fn writes_values_back_as_shell_text() {
        let src = "{ _id: ObjectId(\"65a1b2c3d4e5f60718293a4b\"), \"a-b\": [1, 2.5, NumberLong(7)], \
                   at: ISODate(\"2024-01-02T00:00:00Z\"), d: NumberDecimal(\"1.10\"), r: /x/i, n: null, e: {} }";
        let parsed = value(src);
        assert_eq!(to_shell(&parsed), src.replace("                   ", ""));
        assert_eq!(value(&to_shell(&parsed)), parsed);
        assert_eq!(to_shell(&Bson::Double(3.0)), "3.0");
    }

    #[test]
    fn reports_errors_with_offsets() {
        assert_eq!(error("{ a: 1 ").offset, 7);
        assert_eq!(error("{ a 1 }").message, "expected `:`");
        assert_eq!(error("[1, foo]").offset, 4);
        assert_eq!(error("'abc").message, "unterminated string");
        assert_eq!(error("ObjectId('zz')").offset, 0);
        assert!(error("Nope(1)").message.contains("unknown function"));
    }
}
