//! Scheduled tasks: scripts, exports and backups that run at set times while the app is open.
//! A time that passed while it was closed runs once at the next start. The tasks, their
//! last runs and when each runs next are kept in `schedules.json` in the data folder; the
//! runs themselves are in `commands::schedules`.

pub mod plan;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use plan::{Run, Schedule};

/// The runs of a task kept, the newest.
const HISTORY: usize = 20;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    schedules: Vec<Schedule>,
    #[serde(default)]
    runs: HashMap<Uuid, Vec<Run>>,
    /// When each task runs next at its time.
    #[serde(default)]
    due: HashMap<Uuid, DateTime<Utc>>,
    /// A failed task tried again: when, and which try it is.
    #[serde(default)]
    retry: HashMap<Uuid, (DateTime<Utc>, u32)>,
}

/// A task as the list shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleView {
    #[serde(flatten)]
    pub schedule: Schedule,
    /// When it runs next: its time, or a retry's.
    pub next: Option<DateTime<Utc>>,
    /// Its runs, the newest first.
    pub runs: Vec<Run>,
    pub running: bool,
}

#[derive(Debug)]
pub struct Scheduler {
    /// `None` without a data folder: the tasks then live until the app closes.
    path: Option<PathBuf>,
    saved: Mutex<Saved>,
    running: Mutex<HashSet<Uuid>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Scheduler {
    /// The tasks saved at `path`; none when it does not exist or cannot be read.
    pub fn open(path: Option<PathBuf>) -> Self {
        let saved = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| match serde_json::from_str(&text) {
                Ok(saved) => Some(saved),
                Err(e) => {
                    crate::applog::write("WARN", &format!("schedules.json cannot be read: {e}"));
                    None
                }
            })
            .unwrap_or_default();
        Self {
            path,
            saved: Mutex::new(saved),
            running: Mutex::new(HashSet::new()),
        }
    }

    fn write(&self, saved: &Saved) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
        // Written aside, then moved: a crash halfway leaves the old file whole.
        let aside = path.with_extension("json.tmp");
        std::fs::write(&aside, text)
            .and_then(|()| std::fs::rename(&aside, path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn list(&self) -> Vec<ScheduleView> {
        let saved = lock(&self.saved);
        let running = lock(&self.running);
        saved
            .schedules
            .iter()
            .map(|s| {
                let due = saved.due.get(&s.id).copied();
                let retry = saved.retry.get(&s.id).map(|(at, _)| *at);
                let next = s.enabled.then(|| match (due, retry) {
                    (Some(d), Some(r)) => Some(d.min(r)),
                    (d, r) => d.or(r),
                });
                ScheduleView {
                    schedule: s.clone(),
                    next: next.flatten(),
                    runs: saved.runs.get(&s.id).cloned().unwrap_or_default(),
                    running: running.contains(&s.id),
                }
            })
            .collect()
    }

    pub fn get(&self, id: Uuid) -> Option<Schedule> {
        lock(&self.saved)
            .schedules
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    /// Saves `schedule`, new or changed; it runs next at its first time from `now`.
    pub fn put(&self, schedule: Schedule, now: DateTime<Utc>) -> Result<(), String> {
        if let Some(problem) = schedule.problem() {
            return Err(problem);
        }
        let mut saved = lock(&self.saved);
        let next = schedule.next_after(now).ok_or("the task would never run")?;
        saved.due.insert(schedule.id, next);
        saved.retry.remove(&schedule.id);
        match saved.schedules.iter_mut().find(|s| s.id == schedule.id) {
            Some(old) => *old = schedule,
            None => saved.schedules.push(schedule),
        }
        self.write(&saved)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), String> {
        let mut saved = lock(&self.saved);
        saved.schedules.retain(|s| s.id != id);
        saved.runs.remove(&id);
        saved.due.remove(&id);
        saved.retry.remove(&id);
        self.write(&saved)
    }

    /// The enabled tasks whose time (or retry) has come by `now`, with which try each is;
    /// each runs next at its first time after `now`.
    pub fn take_due(&self, now: DateTime<Utc>) -> Vec<(Schedule, u32)> {
        let mut saved = lock(&self.saved);
        let mut out = Vec::new();
        let schedules = saved.schedules.clone();
        for s in schedules.into_iter().filter(|s| s.enabled) {
            let retry = saved.retry.get(&s.id).copied().filter(|(at, _)| *at <= now);
            // A task without a time yet waits for its first.
            let due = saved.due.get(&s.id).is_some_and(|at| *at <= now);
            if (due || !saved.due.contains_key(&s.id))
                && let Some(next) = s.next_after(now)
            {
                saved.due.insert(s.id, next);
            }
            match retry {
                Some((_, attempt)) => {
                    saved.retry.remove(&s.id);
                    out.push((s, attempt));
                }
                None if due => out.push((s, 1)),
                None => {}
            }
        }
        if !out.is_empty() {
            let _ = self.write(&saved);
        }
        out
    }

    /// Marks `id` as running; `false` when it already is.
    pub fn begin(&self, id: Uuid) -> bool {
        lock(&self.running).insert(id)
    }

    /// Records `run` of `schedule`, which ran as try `attempt`: a failed run at its time is
    /// tried again `retry_minutes` later, up to `retries` times.
    pub fn finish(&self, schedule: &Schedule, run: Run) {
        lock(&self.running).remove(&schedule.id);
        let mut saved = lock(&self.saved);
        if !saved.schedules.iter().any(|s| s.id == schedule.id) {
            return;
        }
        if !run.ok && !run.manual && run.attempt <= schedule.retries {
            let at = run.finished + Duration::minutes(i64::from(schedule.retry_minutes.max(1)));
            saved.retry.insert(schedule.id, (at, run.attempt + 1));
        } else {
            saved.retry.remove(&schedule.id);
        }
        let runs = saved.runs.entry(schedule.id).or_default();
        runs.insert(0, run);
        runs.truncate(HISTORY);
        let _ = self.write(&saved);
    }
}

#[cfg(test)]
mod tests {
    use super::plan::{Repeat, Task};
    use super::*;

    fn schedule(retries: u32) -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: "Refresh".to_owned(),
            enabled: true,
            connection_id: Uuid::nil(),
            database: None,
            task: Task::Script {
                path: PathBuf::from("/scripts/refresh.sql"),
            },
            repeat: Repeat::Day,
            time: "07:00".to_owned(),
            timezone: "UTC".to_owned(),
            folder: None,
            keep: 0,
            retries,
            retry_minutes: 10,
        }
    }

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn run(at: DateTime<Utc>, ok: bool, attempt: u32) -> Run {
        Run {
            started: at,
            finished: at,
            ok,
            message: String::new(),
            file: None,
            attempt,
            manual: false,
        }
    }

    #[test]
    fn a_task_runs_at_its_time_then_waits_for_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schedules.json");
        let scheduler = Scheduler::open(Some(path.clone()));
        let s = schedule(0);
        scheduler
            .put(s.clone(), utc("2026-11-10T06:00:00Z"))
            .unwrap();
        assert!(
            scheduler.take_due(utc("2026-11-10T06:59:00Z")).is_empty(),
            "not yet"
        );
        let due = scheduler.take_due(utc("2026-11-10T07:00:10Z"));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1, 1, "the first try");
        assert!(
            scheduler.take_due(utc("2026-11-10T07:00:30Z")).is_empty(),
            "once"
        );
        assert_eq!(scheduler.list()[0].next, Some(utc("2026-11-11T07:00:00Z")));

        // Kept for the next start; a time passed while closed runs then.
        let reopened = Scheduler::open(Some(path));
        assert_eq!(reopened.list()[0].schedule, s);
        assert_eq!(reopened.take_due(utc("2026-11-12T09:00:00Z")).len(), 1);
        assert_eq!(reopened.list()[0].next, Some(utc("2026-11-13T07:00:00Z")));
    }

    #[test]
    fn a_failed_run_is_tried_again() {
        let scheduler = Scheduler::open(None);
        let s = schedule(1);
        scheduler
            .put(s.clone(), utc("2026-11-10T06:00:00Z"))
            .unwrap();
        let (due, attempt) = scheduler.take_due(utc("2026-11-10T07:00:00Z")).remove(0);
        assert!(scheduler.begin(due.id));
        assert!(!scheduler.begin(due.id), "running already");
        scheduler.finish(&due, run(utc("2026-11-10T07:00:05Z"), false, attempt));
        let view = &scheduler.list()[0];
        assert_eq!(
            view.next,
            Some(utc("2026-11-10T07:10:05Z")),
            "the retry comes first"
        );
        assert!(!view.running);
        let retried = scheduler.take_due(utc("2026-11-10T07:10:05Z"));
        assert_eq!(retried[0].1, 2);
        // Out of retries: the next is its time again.
        scheduler.finish(&s, run(utc("2026-11-10T07:10:06Z"), false, 2));
        assert_eq!(scheduler.list()[0].next, Some(utc("2026-11-11T07:00:00Z")));
        assert_eq!(scheduler.list()[0].runs.len(), 2);
        assert!(!scheduler.list()[0].runs[0].ok);
    }

    #[test]
    fn a_disabled_or_wrong_task_does_not_run() {
        let scheduler = Scheduler::open(None);
        let mut s = schedule(0);
        s.time = "seven".to_owned();
        assert!(
            scheduler
                .put(s.clone(), utc("2026-11-10T06:00:00Z"))
                .is_err()
        );
        s.time = "07:00".to_owned();
        s.enabled = false;
        scheduler
            .put(s.clone(), utc("2026-11-10T06:00:00Z"))
            .unwrap();
        assert_eq!(scheduler.take_due(utc("2026-11-11T08:00:00Z")).len(), 0);
        assert_eq!(scheduler.list()[0].next, None);
        scheduler.delete(s.id).unwrap();
        assert!(scheduler.list().is_empty());
    }
}
