//! The shapes text values take: emails, dates, numbers stored as text and so on. They tell a
//! data-quality story that the type of a column hides: a `VARCHAR` of dates with one `n/a`.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

/// A format's share of a column is reported from this fraction of its values.
pub const SHARE_MIN: f64 = 0.05;
/// A format is the dominant one of a column from this fraction of its values.
pub const DOMINANT_MIN: f64 = 0.5;
/// A column is "mostly" one format from this fraction: the values that differ are shown.
pub const MOSTLY_MIN: f64 = 0.8;
/// Values that do not fit the dominant format, shown at most.
pub const EXCEPTIONS: usize = 5;

/// What a text value looks like. The first eight are exclusive formats (a value has at most one,
/// the first that fits); the last two are properties that come on top of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Email,
    Url,
    Uuid,
    /// `2024-03-31`, optionally with a time.
    IsoDate,
    /// `31/03/2024`, `3.31.24`, `2024/03/31`: a date in a local style.
    DateLike,
    /// A whole number stored as text.
    Integer,
    /// A number with `.` or `,` as its decimal separator, stored as text.
    Decimal,
    Phone,
    /// Spaces or tabs before or after the value.
    Spaces,
    /// The same value, ignoring case, is written in more than one way in the column.
    MixedCase,
}

/// The formats, in the order the first that fits wins.
const FORMATS: [Pattern; 8] = [
    Pattern::Uuid,
    Pattern::Email,
    Pattern::Url,
    Pattern::IsoDate,
    Pattern::DateLike,
    Pattern::Integer,
    Pattern::Decimal,
    Pattern::Phone,
];

/// The source of the regular expressions that tell each format.
const SOURCES: [(Pattern, &[&str]); 8] = [
    (
        Pattern::Uuid,
        &[r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"],
    ),
    (
        Pattern::Email,
        &[r"^[^\s@]+@[^\s@.]+(\.[^\s@.]+)*\.[A-Za-z]{2,}$"],
    ),
    (Pattern::Url, &[r"^(?i:https?|ftp)://[^\s/?#]+\S*$"]),
    (
        Pattern::IsoDate,
        &[
            r"^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])([T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?)?$",
        ],
    ),
    (
        Pattern::DateLike,
        &[
            r"^(0?[1-9]|[12]\d|3[01])[/.-](0?[1-9]|1[0-2]|[12]\d|3[01])[/.-](\d{4}|\d{2})$",
            r"^\d{4}[/.](0?[1-9]|1[0-2])[/.](0?[1-9]|[12]\d|3[01])$",
        ],
    ),
    (Pattern::Integer, &[r"^[+-]?\d+$"]),
    (
        Pattern::Decimal,
        &[
            r"^[+-]?(\d+|\d{1,3}(,\d{3})+)?\.\d+$",
            r"^[+-]?(\d+|\d{1,3}(\.\d{3})+)?,\d+$",
        ],
    ),
    (
        Pattern::Phone,
        &[r"^\+?[\d\s().-]{7,20}$", r"^\d{3}[-.]\d{3}[-.]\d{4}$"],
    ),
];

/// The compiled [`SOURCES`], compiled once.
fn regexes() -> &'static HashMap<Pattern, Vec<Regex>> {
    static COMPILED: LazyLock<HashMap<Pattern, Vec<Regex>>> = LazyLock::new(|| {
        SOURCES
            .into_iter()
            .map(|(pattern, sources)| {
                let compiled = sources
                    .iter()
                    .map(|s| Regex::new(s).expect("the pattern is a constant"))
                    .collect();
                (pattern, compiled)
            })
            .collect()
    });
    &COMPILED
}

impl Pattern {
    /// Whether `value` fits this format; the two properties are not formats and never fit.
    fn fits(self, value: &str) -> bool {
        let Some(found) = regexes().get(&self) else {
            return false;
        };
        let matched = found.iter().any(|r| r.is_match(value));
        if self == Pattern::Integer {
            // `+393331234567` is an E.164 phone number (a `+` and 8 to 15 digits), not a
            // signed integer; `Phone` takes it, as `fits` is tried in order.
            let digits = value.strip_prefix('+').map_or(0, str::len);
            return matched && !(value.starts_with('+') && (8..=15).contains(&digits));
        }
        if self == Pattern::Phone {
            // A `+`, a space or a bracket tells a phone number from a plain integer or a
            // malformed date; seven digits at least. `555-123-4567` is the one dashed shape.
            let digits = value.chars().filter(char::is_ascii_digit).count();
            let marked = value.starts_with('+') || value.contains([' ', '(']);
            let dashed = found.get(1).is_some_and(|r| r.is_match(value));
            return (matched && digits >= 7 && marked) || dashed;
        }
        matched
    }

    /// Position among the formats, to order equal counts the same way every time.
    fn rank(self) -> usize {
        FORMATS
            .iter()
            .position(|&p| p == self)
            .unwrap_or(FORMATS.len())
    }
}

/// The format of `value`, the first of [`FORMATS`] that fits; `None` for free text.
#[must_use]
pub fn classify(value: &str) -> Option<Pattern> {
    FORMATS.into_iter().find(|p| p.fits(value))
}

/// How many values show a pattern.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PatternShare {
    pub pattern: Pattern,
    pub count: usize,
    /// Of the values looked at (NULL and empty ones left out), from 0 to 1.
    pub share: f64,
}

/// The patterns of a text column.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PatternReport {
    /// Values looked at: not NULL and not empty.
    pub checked: usize,
    /// The format that at least half of the values have, if any.
    pub dominant: Option<Pattern>,
    /// Patterns with a share of at least [`SHARE_MIN`], formats first, most common first; the
    /// two properties are listed as soon as one value shows them.
    pub shares: Vec<PatternShare>,
    /// When the column is mostly in the dominant format, a few values that are not.
    pub exceptions: Vec<String>,
}

/// Counts what the values of `texts` look like; empty ones are skipped.
#[must_use]
pub fn report<'a>(texts: impl Iterator<Item = &'a str>) -> PatternReport {
    let mut counter = PatternCounter::default();
    for value in texts {
        counter.add(value);
    }
    counter.finish()
}

/// [`report`] fed one value at a time, so that a column is never held in memory whole: it keeps
/// counts, the distinct spellings of each value (for the mixed case) and a few misfits.
#[derive(Debug, Default)]
pub struct PatternCounter {
    checked: usize,
    formats: HashMap<Pattern, usize>,
    spaces: usize,
    /// The lowercase form of each value: how many times it came, the first way it was written
    /// and whether any other way was seen.
    cases: HashMap<String, Case>,
    /// For each format, the first distinct values that are not in it: the exceptions, should it
    /// turn out to be the dominant one.
    misfits: HashMap<Pattern, Vec<String>>,
}

/// The spellings of one value, ignoring case.
#[derive(Debug)]
struct Case {
    count: usize,
    first: String,
    varied: bool,
}

impl PatternCounter {
    /// Looks at one value; an empty one is skipped.
    pub fn add(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        self.checked += 1;
        let format = classify(value);
        if let Some(f) = format {
            *self.formats.entry(f).or_default() += 1;
        }
        if value.trim_matches([' ', '\t']).len() != value.len() {
            self.spaces += 1;
        }
        self.add_case(value);
        for dominant in FORMATS {
            let found = self.misfits.entry(dominant).or_default();
            if found.len() < EXCEPTIONS
                && format != Some(dominant)
                && !found.iter().any(|f| f == value)
            {
                found.push(value.to_owned());
            }
        }
    }

    /// Counts `value` under its lowercase form.
    fn add_case(&mut self, value: &str) {
        let lower = value.to_lowercase();
        match self.cases.get_mut(&lower) {
            Some(case) => {
                case.count += 1;
                case.varied |= case.first != value;
            }
            None => {
                self.cases.insert(
                    lower,
                    Case {
                        count: 1,
                        first: value.to_owned(),
                        varied: false,
                    },
                );
            }
        }
    }

    /// The report of the values seen.
    #[must_use]
    pub fn finish(self) -> PatternReport {
        let checked = self.checked;
        let mixed: usize = self
            .cases
            .values()
            .filter(|c| c.varied)
            .map(|c| c.count)
            .sum();
        let mut out = PatternReport {
            checked,
            ..PatternReport::default()
        };
        if checked == 0 {
            return out;
        }
        #[expect(clippy::cast_precision_loss, reason = "a share needs no exact count")]
        let share = |count: usize| count as f64 / checked as f64;
        out.shares = FORMATS
            .into_iter()
            .filter_map(|pattern| {
                let count = self.formats.get(&pattern).copied()?;
                (share(count) >= SHARE_MIN).then(|| PatternShare {
                    pattern,
                    count,
                    share: share(count),
                })
            })
            .collect();
        out.shares.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.pattern.rank().cmp(&b.pattern.rank()))
        });
        out.dominant = out
            .shares
            .first()
            .filter(|s| s.share >= DOMINANT_MIN)
            .map(|s| s.pattern);
        for (pattern, count) in [(Pattern::Spaces, self.spaces), (Pattern::MixedCase, mixed)] {
            if count > 0 {
                out.shares.push(PatternShare {
                    pattern,
                    count,
                    share: share(count),
                });
            }
        }
        if let Some(dominant) = out.dominant {
            let top = self.formats.get(&dominant).copied().unwrap_or(0);
            if share(top) >= MOSTLY_MIN && top < checked {
                out.exceptions = self.misfits.get(&dominant).cloned().unwrap_or_default();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(values: &[&str]) -> PatternReport {
        report(values.iter().copied())
    }

    #[test]
    fn classifies_each_format() {
        let cases = [
            ("ada@example.com", Some(Pattern::Email)),
            ("first.last+tag@sub.example.org", Some(Pattern::Email)),
            ("not-an-email", None),
            ("a@b", None),
            ("two@@example.com", None),
            ("https://example.com/a?b=1", Some(Pattern::Url)),
            ("HTTP://EXAMPLE.COM", Some(Pattern::Url)),
            ("example.com", None),
            ("123e4567-e89b-12d3-a456-426614174000", Some(Pattern::Uuid)),
            ("123e4567-e89b-12d3-a456-42661417400", None),
            ("2024-03-31", Some(Pattern::IsoDate)),
            ("2024-03-31T10:20:30Z", Some(Pattern::IsoDate)),
            ("2024-03-31 10:20:30.123+02:00", Some(Pattern::IsoDate)),
            ("2024-13-31", None),
            ("31/03/2024", Some(Pattern::DateLike)),
            ("3.1.24", Some(Pattern::DateLike)),
            ("2024/03/31", Some(Pattern::DateLike)),
            ("42", Some(Pattern::Integer)),
            ("-7", Some(Pattern::Integer)),
            ("+7", Some(Pattern::Integer)),
            ("+1234567", Some(Pattern::Integer)),
            ("+393331234567", Some(Pattern::Phone)),
            ("+12345678", Some(Pattern::Phone)),
            ("+1234567890123456", Some(Pattern::Integer)),
            ("3.14", Some(Pattern::Decimal)),
            ("3,14", Some(Pattern::Decimal)),
            ("1,234.56", Some(Pattern::Decimal)),
            ("1.234,56", Some(Pattern::Decimal)),
            (".5", Some(Pattern::Decimal)),
            ("1.2.3", None),
            ("+39 02 1234567", Some(Pattern::Phone)),
            ("(555) 123-4567", Some(Pattern::Phone)),
            ("12-34", None),
            ("555-123-4567", Some(Pattern::Phone)),
            ("hello", None),
        ];
        for (value, expected) in cases {
            assert_eq!(classify(value), expected, "{value}");
        }
    }

    #[test]
    fn dominant_format_and_exceptions() {
        let values = [
            "a@x.com", "b@x.com", "c@x.com", "d@x.com", "e@x.com", "f@x.com", "g@x.com", "h@x.com",
            "i@x.com", "oops",
        ];
        let r = of(&values);
        assert_eq!(r.checked, 10);
        assert_eq!(r.dominant, Some(Pattern::Email));
        assert_eq!(r.shares[0].count, 9);
        assert!((r.shares[0].share - 0.9).abs() < 1e-9);
        assert_eq!(r.exceptions, ["oops"]);
    }

    #[test]
    fn a_pure_column_has_no_exceptions_and_a_mixed_one_no_dominant() {
        let pure = of(&["1", "2", "3"]);
        assert_eq!(pure.dominant, Some(Pattern::Integer));
        assert_eq!(pure.exceptions, Vec::<String>::new());
        let mixed = of(&["a@x.com", "2024-01-01", "hello", "world", "42", "foo"]);
        assert_eq!(mixed.dominant, None);
        assert_eq!(mixed.exceptions, Vec::<String>::new());
        // Free text only: nothing to report.
        let free = of(&["hello", "world"]);
        assert_eq!(free.shares, Vec::<PatternShare>::new());
        assert_eq!(free.dominant, None);
    }

    #[test]
    fn exceptions_are_distinct_and_capped() {
        let mut values = vec!["1"; 94];
        values.extend(["x", "x", "a", "b", "c", "d"]);
        let r = of(&values);
        assert_eq!(r.dominant, Some(Pattern::Integer));
        assert_eq!(r.exceptions, ["x", "a", "b", "c", "d"]);
    }

    #[test]
    fn spaces_and_mixed_case_are_properties() {
        let r = of(&[" a", "b ", "Paris", "paris", "PARIS", "Rome"]);
        let get = |p| r.shares.iter().find(|s| s.pattern == p).map(|s| s.count);
        assert_eq!(get(Pattern::Spaces), Some(2));
        assert_eq!(get(Pattern::MixedCase), Some(3));
        // One occurrence is already worth a hint, however small its share.
        let mut big = vec!["y"; 99];
        big.push(" x");
        let patterns: Vec<Pattern> = of(&big).shares.iter().map(|s| s.pattern).collect();
        assert_eq!(patterns, [Pattern::Spaces]);
    }

    #[test]
    fn empty_values_are_skipped() {
        let r = of(&["", "", "7"]);
        assert_eq!(r.checked, 1);
        assert_eq!(r.dominant, Some(Pattern::Integer));
        assert_eq!(of(&[]), PatternReport::default());
        assert_eq!(of(&["", ""]).checked, 0);
    }

    #[test]
    fn small_shares_are_not_listed() {
        let mut values = vec!["hello"; 97];
        values.extend(["1", "2", "3"]);
        assert_eq!(of(&values).shares, Vec::<PatternShare>::new());
        let mut values = vec!["hello"; 90];
        values.extend(["1"; 10]);
        assert_eq!(of(&values).shares[0].pattern, Pattern::Integer);
    }
}
