//! The user's display formats for the grid: dates and timestamps, the time zone of zoned
//! timestamps, digit grouping and relative times. Only the grid's text follows them; editing,
//! copying, searching and exports keep the plain values of `display::format_options`.
//! The keys are declared in the UI, in `apps/desktop/src/grid/formats.ts`.

use std::fmt::Write as _;

use arrow_array::cast::AsArray;
use arrow_array::types::TimestampMicrosecondType;
use arrow_array::{Array, ArrayRef};
use arrow_schema::{ArrowError, DataType, TimeUnit};
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Local, TimeZone, Utc};
use platypus_store::SettingsStore;

pub const DATE_FORMAT: &str = "%Y-%m-%d";
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f";

/// Where zoned timestamps are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// In the zone the database returned them in.
    AsReturned,
    /// In this computer's zone, with its daylight saving rules at each instant.
    Local,
    Utc,
}

/// How numbers group their digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grouping {
    pub group: char,
    pub decimal: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayFormat {
    pub date: String,
    pub timestamp: String,
    /// `timestamp` with the offset, for zoned timestamps.
    pub timestamp_tz: String,
    pub zone: Zone,
    pub grouping: Option<Grouping>,
    /// Timestamps as "3 hours ago"; timestamps without a zone are taken as local time.
    pub relative: bool,
}

impl Default for DisplayFormat {
    fn default() -> Self {
        Self::new(DATE_FORMAT, TIMESTAMP_FORMAT, Zone::AsReturned, None, false)
    }
}

/// A `strftime` pattern chrono accepts, else `fallback`: a bad pattern would fail every row.
fn pattern(text: &str, fallback: &str) -> String {
    let valid =
        !text.trim().is_empty() && !StrftimeItems::new(text).any(|i| matches!(i, Item::Error));
    if valid { text } else { fallback }.to_owned()
}

impl DisplayFormat {
    #[must_use]
    pub fn new(
        date: &str,
        timestamp: &str,
        zone: Zone,
        grouping: Option<Grouping>,
        relative: bool,
    ) -> Self {
        let timestamp = pattern(timestamp, TIMESTAMP_FORMAT);
        Self {
            date: pattern(date, DATE_FORMAT),
            timestamp_tz: format!("{timestamp}%:z"),
            timestamp,
            zone,
            grouping,
            relative,
        }
    }

    #[must_use]
    pub fn from_settings(s: &SettingsStore) -> Self {
        let text = |key: &str| s.get::<String>(key).unwrap_or_default();
        let zone = match text("results.time_zone").as_str() {
            "local" => Zone::Local,
            "utc" => Zone::Utc,
            _ => Zone::AsReturned,
        };
        let grouping = match text("results.number_format").as_str() {
            "comma" => Some(Grouping {
                group: ',',
                decimal: '.',
            }),
            "dot" => Some(Grouping {
                group: '.',
                decimal: ',',
            }),
            "space" => Some(Grouping {
                group: '\u{202f}',
                decimal: ',',
            }),
            "apostrophe" => Some(Grouping {
                group: '\'',
                decimal: '.',
            }),
            _ => None,
        };
        Self::new(
            &text("results.date_format"),
            &text("results.timestamp_format"),
            zone,
            grouping,
            s.get::<bool>("results.relative_time").unwrap_or(false),
        )
    }

    /// Whether timestamps of this type are rendered here rather than by Arrow.
    #[must_use]
    pub fn own_timestamps(&self, data_type: &DataType) -> bool {
        match data_type {
            DataType::Timestamp(_, tz) => {
                self.relative || (tz.is_some() && self.zone != Zone::AsReturned)
            }
            _ => false,
        }
    }

    /// Calls `cell` with the text of every non-NULL value of a timestamp array.
    pub fn timestamps(
        &self,
        array: &ArrayRef,
        mut cell: impl FnMut(usize, &str),
    ) -> Result<(), ArrowError> {
        let DataType::Timestamp(_, tz) = array.data_type() else {
            return Ok(());
        };
        let zoned = tz.is_some();
        let micros = arrow_cast::cast(
            array,
            &DataType::Timestamp(TimeUnit::Microsecond, tz.clone()),
        )?;
        let now = Utc::now();
        let mut text = String::new();
        for (i, v) in micros
            .as_primitive::<TimestampMicrosecondType>()
            .iter()
            .enumerate()
        {
            let Some(at) = v.and_then(DateTime::from_timestamp_micros) else {
                continue;
            };
            text.clear();
            if self.relative {
                // Without a zone the value is a wall-clock time: take it as local time.
                let instant = if zoned {
                    Some(at)
                } else {
                    Local
                        .from_local_datetime(&at.naive_utc())
                        .earliest()
                        .map(|t| t.with_timezone(&Utc))
                };
                match instant {
                    Some(instant) => text.push_str(&relative((now - instant).num_seconds())),
                    None => continue,
                }
            } else {
                let _ = match self.zone {
                    Zone::Local => write!(
                        text,
                        "{}",
                        at.with_timezone(&Local).format(&self.timestamp_tz)
                    ),
                    _ => write!(text, "{}", at.format(&self.timestamp_tz)),
                };
            }
            cell(i, &text);
        }
        Ok(())
    }

    /// Groups the digits of a number as Arrow printed it, e.g. `-1234567.5` → `-1,234,567.5`.
    #[must_use]
    pub fn number(&self, text: &str) -> Option<String> {
        let g = self.grouping?;
        // Exponents and words (NaN, inf) stay as they are.
        if text.bytes().any(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        let (sign, digits) = text
            .strip_prefix('-')
            .map_or(("", text), |rest| ("-", rest));
        let (int, frac) = digits
            .split_once('.')
            .map_or((digits, None), |(i, f)| (i, Some(f)));
        let mut out = String::with_capacity(text.len() + int.len() / 3 + 1);
        out.push_str(sign);
        for (i, c) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 {
                out.push(g.group);
            }
            out.push(c);
        }
        if let Some(frac) = frac {
            out.push(g.decimal);
            out.push_str(frac);
        }
        Some(out)
    }
}

/// "5 minutes ago", "in 2 days": `seconds` is now minus the time.
fn relative(seconds: i64) -> String {
    let span = seconds.unsigned_abs();
    if span < 45 {
        return "just now".to_owned();
    }
    let (n, unit) = match span {
        s if s < 3_600 => (s.div_ceil(60).max(1), "minute"),
        s if s < 86_400 => (s / 3_600, "hour"),
        s if s < 30 * 86_400 => (s / 86_400, "day"),
        s if s < 365 * 86_400 => (s / (30 * 86_400), "month"),
        s => (s / (365 * 86_400), "year"),
    };
    let plural = if n == 1 { "" } else { "s" };
    if seconds >= 0 {
        format!("{n} {unit}{plural} ago")
    } else {
        format!("in {n} {unit}{plural}")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::TimestampSecondArray;

    use super::*;

    #[test]
    fn groups_digits() {
        let f = DisplayFormat::new(
            "",
            "",
            Zone::AsReturned,
            Some(Grouping {
                group: ',',
                decimal: '.',
            }),
            false,
        );
        assert_eq!(f.number("1234567.25").as_deref(), Some("1,234,567.25"));
        assert_eq!(f.number("-123").as_deref(), Some("-123"));
        assert_eq!(f.number("-1234").as_deref(), Some("-1,234"));
        assert_eq!(f.number("1e30"), None);
        let dot = DisplayFormat::new(
            "",
            "",
            Zone::AsReturned,
            Some(Grouping {
                group: '.',
                decimal: ',',
            }),
            false,
        );
        assert_eq!(dot.number("1234.5").as_deref(), Some("1.234,5"));
        assert_eq!(DisplayFormat::default().number("1234"), None);
    }

    #[test]
    fn falls_back_from_bad_patterns() {
        let f = DisplayFormat::new("%Q", "%d/%m/%Y %H:%M", Zone::Utc, None, false);
        assert_eq!(f.date, DATE_FORMAT);
        assert_eq!(f.timestamp_tz, "%d/%m/%Y %H:%M%:z");
    }

    #[test]
    fn shows_zoned_timestamps_in_utc_and_relative() {
        let array: ArrayRef =
            Arc::new(TimestampSecondArray::from(vec![Some(0), None]).with_timezone("+02:00"));
        let utc = DisplayFormat::new("", "%Y-%m-%d %H:%M", Zone::Utc, None, false);
        let mut out = Vec::new();
        utc.timestamps(&array, |i, t| out.push((i, t.to_owned())))
            .unwrap();
        assert_eq!(out, [(0, "1970-01-01 00:00+00:00".to_owned())]);
        assert_eq!(relative(10), "just now");
        assert_eq!(relative(3 * 3_600), "3 hours ago");
        assert_eq!(relative(-86_400), "in 1 day");
        assert_eq!(relative(90), "2 minutes ago");
    }
}
