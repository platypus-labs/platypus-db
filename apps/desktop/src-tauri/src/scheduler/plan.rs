//! Scheduled tasks: what runs (a script, an export, a backup), on which connection, when,
//! and where its files go. Pure: the times are computed here, the runs in `super`.

use std::path::{Path, PathBuf};

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone, Timelike, Utc,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::commands::backup::BackupFormat;

/// What a scheduled task does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Task {
    /// Runs the statements of a SQL file, stopping at the first that fails.
    Script { path: PathBuf },
    /// Writes the rows of a query to a file in the task's folder.
    Export { sql: String, format: ExportFormat },
    /// Backs the database up to a file in the task's folder (`pg_dump`, `mysqldump`).
    Backup {
        format: BackupFormat,
        #[serde(default)]
        schema: Option<String>,
    },
}

/// The formats of a scheduled export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Json,
    Parquet,
    Xlsx,
}

impl ExportFormat {
    pub fn transfer(self) -> platypus_transfer::Format {
        match self {
            Self::Csv => platypus_transfer::Format::Csv,
            Self::Json => platypus_transfer::Format::Json,
            Self::Parquet => platypus_transfer::Format::Parquet,
            Self::Xlsx => platypus_transfer::Format::Xlsx,
        }
    }
}

/// How often a task runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "every", rename_all = "snake_case")]
pub enum Repeat {
    /// At the minute of `time` of every hour.
    Hour,
    Day,
    /// On these days, 1 for Monday to 7 for Sunday.
    Week {
        days: Vec<u8>,
    },
    /// On this day of the month; the last day in a month without it.
    Month {
        day: u8,
    },
}

/// A scheduled task, as saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub connection_id: Uuid,
    /// Another database of the server than the connection's own.
    #[serde(default)]
    pub database: Option<String>,
    pub task: Task,
    pub repeat: Repeat,
    /// `HH:MM`, in `timezone`.
    pub time: String,
    /// An IANA name (`Europe/Rome`), or empty for the system's.
    #[serde(default)]
    pub timezone: String,
    /// Where an export or a backup writes its files.
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// The files of the task kept in its folder, the newest; 0 keeps all.
    #[serde(default)]
    pub keep: u32,
    /// The tries after a failed run, `retry_minutes` apart.
    #[serde(default)]
    pub retries: u32,
    #[serde(default = "default_retry_minutes")]
    pub retry_minutes: u32,
}

const fn default_retry_minutes() -> u32 {
    5
}

/// One run of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub started: DateTime<Utc>,
    pub finished: DateTime<Utc>,
    pub ok: bool,
    /// What it did, or why it failed.
    pub message: String,
    /// The file it wrote.
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// 1 for the run at its time, 2 for the first retry, …
    pub attempt: u32,
    /// Started with "Run now".
    #[serde(default)]
    pub manual: bool,
}

impl Schedule {
    /// What is wrong with the schedule, if anything: it would never run, or not do its work.
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("the task needs a name".to_owned());
        }
        if parse_time(&self.time).is_none() {
            return Some(format!("{:?} is not a time: write it as HH:MM", self.time));
        }
        if !self.timezone.is_empty() && self.timezone.parse::<chrono_tz::Tz>().is_err() {
            return Some(format!("{:?} is not a time zone", self.timezone));
        }
        match &self.repeat {
            Repeat::Week { days }
                if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) =>
            {
                return Some("choose the days of the week it runs on".to_owned());
            }
            Repeat::Month { day } if !(1..=31).contains(day) => {
                return Some("the day of the month is from 1 to 31".to_owned());
            }
            _ => {}
        }
        match &self.task {
            Task::Script { path } if path.as_os_str().is_empty() => {
                Some("choose the SQL file to run".to_owned())
            }
            Task::Export { sql, .. } if sql.trim().is_empty() => {
                Some("write the query to export".to_owned())
            }
            Task::Export { .. } | Task::Backup { .. } if self.folder.is_none() => {
                Some("choose the folder its files go to".to_owned())
            }
            _ => None,
        }
    }

    /// The first time after `after` that the task runs at; `None` if it never does (a
    /// schedule with a [`problem`](Self::problem)).
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let time = parse_time(&self.time)?;
        if self.timezone.is_empty() {
            next_in(&chrono::Local, &self.repeat, time, after)
        } else {
            next_in(
                &self.timezone.parse::<chrono_tz::Tz>().ok()?,
                &self.repeat,
                time,
                after,
            )
        }
    }

    /// The stem of the task's files, `name-`: what a file name starts with.
    pub fn file_stem(&self) -> String {
        let name: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}-", name.trim_matches('_'))
    }

    /// The file of a run started at `at`, in the task's folder: `name-20261029-0700.ext`.
    pub fn file_at(&self, at: DateTime<Utc>, extension: &str) -> Option<PathBuf> {
        let stamp = at.with_timezone(&chrono::Local).format("%Y%m%d-%H%M%S");
        Some(
            self.folder
                .as_ref()?
                .join(format!("{}{stamp}.{extension}", self.file_stem())),
        )
    }
}

/// `HH:MM` as a time of day.
fn parse_time(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text.trim(), "%H:%M").ok()
}

/// The first time after `after`, in `tz`, at `time` on the days of `repeat`.
fn next_in<Z: TimeZone>(
    tz: &Z,
    repeat: &Repeat,
    time: NaiveTime,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let local = after.with_timezone(tz).naive_local();
    if *repeat == Repeat::Hour {
        let mut at = local.date().and_hms_opt(local.hour(), time.minute(), 0)?;
        loop {
            let instant = at_in(tz, at.date(), at.time())?;
            if instant > after {
                return Some(instant);
            }
            at += Duration::hours(1);
        }
    }
    // A year ahead holds every day of the week and of the month.
    for offset in 0..=400 {
        let day = local.date() + Duration::days(offset);
        let wanted = match repeat {
            Repeat::Hour | Repeat::Day => true,
            Repeat::Week { days } => {
                days.contains(&u8::try_from(day.weekday().number_from_monday()).unwrap_or(0))
            }
            Repeat::Month { day: d } => day.day() == u32::from(*d).min(last_day(day)),
        };
        if wanted
            && let Some(at) = at_in(tz, day, time)
            && at > after
        {
            return Some(at);
        }
    }
    None
}

/// The instant of `day` at `time` in `tz`. A time skipped by the change to summer time runs
/// an hour later; one that happens twice runs the first time.
fn at_in<Z: TimeZone>(tz: &Z, day: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    let at = day.and_time(time);
    match tz.from_local_datetime(&at) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => Some(t.with_timezone(&Utc)),
        LocalResult::None => tz
            .from_local_datetime(&(at + Duration::hours(1)))
            .earliest()
            .map(|t| t.with_timezone(&Utc)),
    }
}

/// The last day of `day`'s month.
fn last_day(day: NaiveDate) -> u32 {
    let (y, m) = if day.month() == 12 {
        (day.year() + 1, 1)
    } else {
        (day.year(), day.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1)
        .and_then(|d| d.pred_opt())
        .map_or(28, |d| d.day())
}

/// The files of `schedule` in `folder` beyond the newest `keep`, oldest first: their names
/// end with a time stamp, so they sort by it.
pub fn surplus(schedule: &Schedule, folder: &Path, extension: &str) -> Vec<PathBuf> {
    if schedule.keep == 0 {
        return Vec::new();
    }
    let stem = schedule.file_stem();
    let suffix = format!(".{extension}");
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with(&stem)
                    && n.ends_with(&suffix)
                    && n[stem.len()..n.len() - suffix.len()]
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '-')
            })
        })
        .collect();
    files.sort();
    let keep = usize::try_from(schedule.keep).unwrap_or(usize::MAX);
    let extra = files.len().saturating_sub(keep);
    files.truncate(extra);
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(repeat: Repeat, time: &str, timezone: &str) -> Schedule {
        Schedule {
            id: Uuid::nil(),
            name: "Nightly backup".to_owned(),
            enabled: true,
            connection_id: Uuid::nil(),
            database: None,
            task: Task::Backup {
                format: BackupFormat::Custom,
                schema: None,
            },
            repeat,
            time: time.to_owned(),
            timezone: timezone.to_owned(),
            folder: Some(PathBuf::from("/backups")),
            keep: 3,
            retries: 0,
            retry_minutes: 5,
        }
    }

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn runs_every_day_at_its_time_in_its_zone() {
        let s = schedule(Repeat::Day, "07:00", "Europe/Rome");
        // 06:30 in Rome (UTC+1 in winter): today at 7.
        assert_eq!(
            s.next_after(utc("2026-11-10T05:30:00Z")),
            Some(utc("2026-11-10T06:00:00Z"))
        );
        // Exactly at 7: tomorrow.
        assert_eq!(
            s.next_after(utc("2026-11-10T06:00:00Z")),
            Some(utc("2026-11-11T06:00:00Z"))
        );
        // In summer Rome is UTC+2.
        assert_eq!(
            s.next_after(utc("2026-07-01T00:00:00Z")),
            Some(utc("2026-07-01T05:00:00Z"))
        );
    }

    #[test]
    fn a_time_skipped_by_summer_time_runs_an_hour_later() {
        // On 29 March 2026 Rome's clocks go from 02:00 to 03:00.
        let s = schedule(Repeat::Day, "02:30", "Europe/Rome");
        assert_eq!(
            s.next_after(utc("2026-03-28T12:00:00Z")),
            Some(utc("2026-03-29T01:30:00Z"))
        );
    }

    #[test]
    fn runs_on_the_days_of_the_week_and_of_the_month() {
        // Monday and Thursday at 22:15 UTC; 2026-11-10 is a Tuesday.
        let s = schedule(Repeat::Week { days: vec![1, 4] }, "22:15", "UTC");
        assert_eq!(
            s.next_after(utc("2026-11-10T10:00:00Z")),
            Some(utc("2026-11-12T22:15:00Z"))
        );
        assert_eq!(
            s.next_after(utc("2026-11-12T22:15:00Z")),
            Some(utc("2026-11-16T22:15:00Z"))
        );
        // The 31st, on the last day of shorter months.
        let s = schedule(Repeat::Month { day: 31 }, "01:00", "UTC");
        assert_eq!(
            s.next_after(utc("2026-02-01T00:00:00Z")),
            Some(utc("2026-02-28T01:00:00Z"))
        );
        assert_eq!(
            s.next_after(utc("2026-02-28T01:00:00Z")),
            Some(utc("2026-03-31T01:00:00Z"))
        );
    }

    #[test]
    fn runs_every_hour_at_its_minute() {
        let s = schedule(Repeat::Hour, "00:20", "UTC");
        assert_eq!(
            s.next_after(utc("2026-11-10T10:05:00Z")),
            Some(utc("2026-11-10T10:20:00Z"))
        );
        assert_eq!(
            s.next_after(utc("2026-11-10T10:20:00Z")),
            Some(utc("2026-11-10T11:20:00Z"))
        );
        assert_eq!(
            s.next_after(utc("2026-11-10T23:59:00Z")),
            Some(utc("2026-11-11T00:20:00Z"))
        );
    }

    #[test]
    fn says_what_is_wrong() {
        assert_eq!(schedule(Repeat::Day, "07:00", "").problem(), None);
        assert!(
            schedule(Repeat::Day, "7am", "")
                .problem()
                .unwrap()
                .contains("HH:MM")
        );
        assert!(
            schedule(Repeat::Day, "07:00", "Mars/Olympus")
                .problem()
                .unwrap()
                .contains("time zone")
        );
        assert!(
            schedule(Repeat::Week { days: vec![] }, "07:00", "")
                .problem()
                .is_some()
        );
        assert!(
            schedule(Repeat::Month { day: 32 }, "07:00", "")
                .problem()
                .is_some()
        );
        let mut s = schedule(Repeat::Day, "07:00", "");
        s.folder = None;
        assert!(s.problem().unwrap().contains("folder"));
        assert!(s.next_after(utc("2026-11-10T00:00:00Z")).is_some());
    }

    #[test]
    fn keeps_the_newest_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = schedule(Repeat::Day, "07:00", "");
        s.folder = Some(dir.path().to_owned());
        s.keep = 2;
        for stamp in ["20261101-070000", "20261102-070000", "20261103-070000"] {
            std::fs::write(dir.path().join(format!("Nightly_backup-{stamp}.dump")), "x").unwrap();
        }
        // Another task's file and one by hand stay.
        std::fs::write(dir.path().join("Nightly_backup-old copy.dump"), "x").unwrap();
        std::fs::write(dir.path().join("Other-20261101-070000.dump"), "x").unwrap();
        let surplus = surplus(&s, dir.path(), "dump");
        assert_eq!(
            surplus,
            [dir.path().join("Nightly_backup-20261101-070000.dump")]
        );
        s.keep = 0;
        assert!(
            super::surplus(&s, dir.path(), "dump").is_empty(),
            "0 keeps all"
        );
        let file = s.file_at(utc("2026-11-10T06:00:00Z"), "dump").unwrap();
        assert!(
            file.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("Nightly_backup-2026111")
        );
    }
}
