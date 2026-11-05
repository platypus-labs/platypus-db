//! What a connection's agent limits ask of the tools beyond the fixed caps of [`Limits`]:
//! rows, bytes and timeout lowered to the connection's, a sliding window of calls per minute,
//! and the cost guard that refuses a read whose plan is too big before it runs.
//!
//! The limits only tighten. They come from the sealed policy ([`crate::policy`]) through
//! [`Agent::connections`](crate::Agent), so a hand edit of `connections.toml` cannot raise them.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use platypus_core::{DriverKind, Explain, PlanNode};
use platypus_sql::sensitive::MASKED_VALUE;
use platypus_store::{AgentLimits, AgentValues, ConnectionStore, SavedConnection};
use uuid::Uuid;

use crate::access;
use crate::agent::{Agent, locked};
use crate::error::{AgentError, Result};
use crate::plan::PlanResult;
use crate::query::{self, Limits, RunError};

/// The default limits lowered to the connection's `rules`: for a tool that takes none.
pub(crate) fn default_limits(rules: &AgentLimits) -> Result<Limits> {
    Ok(clamp(Limits::new(None, None, None)?, &Asked::default(), rules).0)
}

/// The limits for a tool that takes only `max_bytes` and `timeout_seconds`, lowered to the
/// connection's `rules`.
pub(crate) fn byte_limits(
    max_bytes: Option<u64>,
    timeout_seconds: Option<f64>,
    rules: &AgentLimits,
) -> Result<Limits> {
    let asked = Asked {
        max_bytes,
        timeout_seconds,
        ..Asked::default()
    };
    let limits = Limits::new(None, max_bytes, timeout_seconds)?;
    Ok(clamp(limits, &asked, rules).0)
}

/// How long calls are counted for `calls_per_minute`.
const WINDOW: Duration = Duration::from_secs(60);

/// What a read says when the cost guard could not look at its plan.
const UNCHECKED: &str = "The connection limits the estimated cost of a read, but this one's plan could not be \
     checked (no estimate available): it ran unchecked.";

/// What the caller asked of `run_query`, before the connection's limits.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Asked {
    pub max_rows: Option<u64>,
    pub max_bytes: Option<u64>,
    pub timeout_seconds: Option<f64>,
}

impl Asked {
    /// The defaults with what was asked, each up to its cap.
    pub(crate) fn limits(&self) -> Result<Limits> {
        Limits::new(self.max_rows, self.max_bytes, self.timeout_seconds)
    }
}

/// `limits` (the defaults with what was `asked`, up to the fixed caps) lowered to the
/// connection's `rules`, and a note for each one the agent asked more than it was given.
/// A default above a connection's limit is lowered without a note.
pub(crate) fn clamp(limits: Limits, asked: &Asked, rules: &AgentLimits) -> (Limits, Vec<String>) {
    let mut out = limits;
    let mut notes = Vec::new();
    if let Some(max) = rules.max_rows {
        let max_usize = usize::try_from(max).unwrap_or(usize::MAX);
        out.max_rows = out.max_rows.min(max_usize);
        if asked.max_rows.is_some_and(|a| a > max) {
            notes.push(format!(
                "max_rows was lowered to {max}, the most this connection gives agents."
            ));
        }
    }
    if let Some(max) = rules.max_bytes {
        let max_usize = usize::try_from(max).unwrap_or(usize::MAX);
        out.max_bytes = out.max_bytes.min(max_usize);
        if asked.max_bytes.is_some_and(|a| a > max) {
            notes.push(format!(
                "max_bytes was lowered to {max}, the most this connection gives agents."
            ));
        }
    }
    if let Some(max) = rules.timeout_seconds {
        out.timeout = out.timeout.min(Duration::from_secs(max));
        if asked
            .timeout_seconds
            .is_some_and(|a| a > Duration::from_secs(max).as_secs_f64())
        {
            notes.push(format!(
                "timeout_seconds was lowered to {max}, the most this connection gives agents."
            ));
        }
    }
    (out, notes)
}

/// Whether a call at `now` fits in `per_minute` calls over the last minute; if so it is
/// counted in `window`, else the wait until the oldest call leaves the window is returned.
pub(crate) fn admit(
    window: &mut VecDeque<Instant>,
    now: Instant,
    per_minute: u64,
) -> Result<(), Duration> {
    while window
        .front()
        .is_some_and(|t| now.saturating_duration_since(*t) >= WINDOW)
    {
        window.pop_front();
    }
    let allowed = usize::try_from(per_minute).unwrap_or(usize::MAX);
    if window.len() >= allowed
        && let Some(oldest) = window.front()
    {
        return Err(WINDOW.saturating_sub(now.saturating_duration_since(*oldest)));
    }
    window.push_back(now);
    Ok(())
}

/// The calls counted for each connection in the last minute, for this process.
#[derive(Default)]
pub(crate) struct CallWindows(Mutex<HashMap<Uuid, VecDeque<Instant>>>);

impl CallWindows {
    /// Counts a call on `saved`, or refuses it when `calls_per_minute` is used up.
    pub(crate) fn count(&self, saved: &SavedConnection) -> Result<()> {
        let Some(per_minute) = saved.agent_limits.calls_per_minute else {
            return Ok(());
        };
        let mut windows = locked(&self.0);
        admit(
            windows.entry(saved.id).or_default(),
            Instant::now(),
            per_minute,
        )
        .map_err(|wait| {
            AgentError::Denied(format!(
                "`{}` allows agents {per_minute} calls a minute and they are used up: \
                 try again in {} seconds",
                saved.name,
                wait.as_secs() + 1
            ))
        })
    }
}

/// What the cost guard made of a plan.
#[derive(Debug, PartialEq)]
enum Verdict {
    Fine,
    /// A limit is set whose figure the plan does not give.
    Unchecked,
    /// The limit that the plan exceeds, in words.
    Over(String),
}

/// Which figure of a plan step a limit is about.
type Figure = fn(&PlanNode) -> Option<f64>;

/// The largest figure the plan gives: the root's, else the largest of the steps'.
fn estimate(root: &PlanNode, figure: Figure) -> Option<f64> {
    fn largest(node: &PlanNode, figure: Figure) -> Option<f64> {
        node.children
            .iter()
            .filter_map(|c| largest(c, figure))
            .chain(figure(node))
            .reduce(f64::max)
    }
    figure(root).or_else(|| largest(root, figure))
}

fn verdict(explain: &Explain, rules: &AgentLimits) -> Verdict {
    let mut unchecked = false;
    let checks: [(&str, Option<u64>, Figure); 2] = [
        ("max_estimated_rows", rules.max_estimated_rows, |n| n.rows),
        ("max_plan_cost", rules.max_plan_cost, |n| n.cost),
    ];
    for (name, limit, figure) in checks {
        let Some(limit) = limit else { continue };
        match estimate(&explain.root, figure) {
            Some(value) if value > to_f64(limit) => {
                let what = if name == "max_plan_cost" {
                    "costs"
                } else {
                    "expects"
                };
                let unit = if name == "max_plan_cost" { "" } else { " rows" };
                return Verdict::Over(format!(
                    "the plan {what} {}{unit}, over this connection's {name} of {limit}",
                    figure_text(value)
                ));
            }
            Some(_) => {}
            None => unchecked = true,
        }
    }
    if unchecked {
        Verdict::Unchecked
    } else {
        Verdict::Fine
    }
}

/// A limit as a figure of a plan: exact up to 2^53, far beyond any plan.
#[allow(clippy::cast_precision_loss)]
fn to_f64(n: u64) -> f64 {
    n as f64
}

fn figure_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n:.2}")
    }
}

/// `line` with its quoted literals written as [`MASKED_VALUE`], and its numbers too when
/// `numbers`.
fn mask_values(line: &str, numbers: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut previous = ' ';
    while let Some(c) = chars.next() {
        if c == '\'' {
            while let Some(inner) = chars.next() {
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            out.push_str(MASKED_VALUE);
        } else if numbers && c.is_ascii_digit() && !(previous.is_alphanumeric() || previous == '_')
        {
            while chars
                .peek()
                .is_some_and(|n| n.is_ascii_digit() || *n == '.')
            {
                chars.next();
            }
            out.push_str(MASKED_VALUE);
        } else {
            out.push(c);
        }
        previous = c;
    }
    out
}

/// A plan's `text` with the values compared to sensitive columns masked: a line that names a
/// sensitive column (`Filter: (email = 'a@b.c')`) loses its literals, and a condition line
/// its numbers too. The figures of the steps stay.
pub(crate) fn mask_plan(text: &str, sensitive: &dyn Fn(&str) -> bool) -> String {
    let names_sensitive = |line: &str| {
        line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|word| !word.is_empty() && sensitive(word))
    };
    text.lines()
        .map(|line| {
            if !names_sensitive(line) {
                return line.to_owned();
            }
            let step = line.trim_start().starts_with("->");
            mask_values(line, !step)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The plan in a few lines for an error: the steps without the closing line.
fn compact(explain: &Explain) -> String {
    let text = PlanResult {
        explain: explain.clone(),
        elapsed: Duration::ZERO,
        max_bytes: 2000,
    }
    .text();
    text.rsplit_once('\n')
        .map_or(text.clone(), |(steps, _)| steps.to_owned())
}

/// Whether statements on `driver` have a plan the cost guard can read.
pub(crate) const fn has_plans(driver: DriverKind) -> bool {
    !matches!(
        driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch | DriverKind::Cassandra
    )
}

impl Agent {
    /// Refuses `statement`, a read, when its estimated plan is over the connection's
    /// `max_estimated_rows` or `max_plan_cost`; the error carries the plan. `Some(note)` when
    /// the plan could not be checked, which does not stop the read.
    pub(crate) async fn cost_guard(
        &self,
        (store, saved): (&ConnectionStore, &SavedConnection),
        statement: &str,
        limits: &Limits,
        stop: impl Future<Output = ()>,
    ) -> Result<Option<String>> {
        let rules = saved.agent_limits;
        if (rules.max_estimated_rows.is_none() && rules.max_plan_cost.is_none())
            || access::changes(statement, saved.driver)
        {
            return Ok(None);
        }
        if !has_plans(saved.driver) {
            return Ok(Some(UNCHECKED.to_owned()));
        }
        let lease = self.lease(store, saved).await?;
        if !lease.conn().capabilities().explain {
            return Ok(Some(UNCHECKED.to_owned()));
        }
        let work = lease.conn().explain(statement, false);
        let planned = query::guarded(lease.conn(), None, limits.timeout, stop, work).await;
        let explain = match planned {
            Ok(explain) => explain,
            Err(RunError::Database(_)) => return Ok(Some(UNCHECKED.to_owned())),
            Err(other) => return lease.finish(Err(other), limits.timeout).map(|()| None),
        };
        drop(lease);
        match verdict(&explain, &rules) {
            Verdict::Fine => Ok(None),
            Verdict::Unchecked => Ok(Some(UNCHECKED.to_owned())),
            Verdict::Over(reason) => {
                let plan = self.mask_plan_text(saved, &compact(&explain))?;
                Err(AgentError::Denied(format!(
                    "`{}` does not run this read: {reason}. Narrow it (a WHERE on an indexed \
                     column, a LIMIT) and try again.\n{plan}",
                    saved.name
                )))
            }
        }
    }

    /// `text`, a plan, as agents read it on `saved`: the values compared to sensitive columns
    /// masked.
    pub(crate) fn mask_plan_text(&self, saved: &SavedConnection, text: &str) -> Result<String> {
        if saved.agent_values == AgentValues::Shown {
            return Ok(text.to_owned());
        }
        let names = self.sensitivity()?.names(&[saved.id])?;
        Ok(mask_plan(text, &|word| names.is(word)))
    }
}

#[cfg(test)]
mod tests {
    use platypus_core::ExplainFormat;

    use super::*;

    fn rules(rows: Option<u64>, bytes: Option<u64>, timeout: Option<u64>) -> AgentLimits {
        AgentLimits {
            max_rows: rows,
            max_bytes: bytes,
            timeout_seconds: timeout,
            ..AgentLimits::default()
        }
    }

    #[test]
    fn what_the_agent_asks_is_lowered_to_the_connection_and_says_so() {
        let asked = Asked {
            max_rows: Some(5000),
            max_bytes: Some(10),
            timeout_seconds: Some(100.0),
        };
        let limits = Limits::new(Some(5000), Some(10), Some(100.0)).unwrap();
        let (out, notes) = clamp(limits, &asked, &rules(Some(500), Some(100_000), Some(20)));
        assert_eq!(out.max_rows, 500);
        assert_eq!(out.max_bytes, 10, "less than the limit is kept");
        assert_eq!(out.timeout, Duration::from_secs(20));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("max_rows") && notes[0].contains("500"));
        assert!(notes[1].contains("timeout_seconds"));
    }

    #[test]
    fn a_default_above_the_limit_is_lowered_without_a_note() {
        let (out, notes) = clamp(
            Limits::default(),
            &Asked::default(),
            &rules(Some(10), None, None),
        );
        assert_eq!(out.max_rows, 10);
        assert_eq!(notes, Vec::<String>::new());
        // No limit: the fixed caps decide, as before.
        let (same, _) = clamp(
            Limits::default(),
            &Asked::default(),
            &AgentLimits::default(),
        );
        assert_eq!(same, Limits::default());
    }

    #[test]
    fn calls_over_the_window_wait_for_the_oldest_to_leave() {
        let start = Instant::now();
        let mut window = VecDeque::new();
        assert!(admit(&mut window, start, 2).is_ok());
        assert!(admit(&mut window, start + Duration::from_secs(10), 2).is_ok());
        let wait = admit(&mut window, start + Duration::from_secs(20), 2).unwrap_err();
        assert_eq!(wait, Duration::from_secs(40));
        assert_eq!(window.len(), 2, "a refused call is not counted");
        // The first leaves at 60 s; the second still counts.
        assert!(admit(&mut window, start + Duration::from_secs(60), 2).is_ok());
        assert!(admit(&mut window, start + Duration::from_secs(61), 2).is_err());
        assert!(admit(&mut window, start + Duration::from_secs(200), 1).is_ok());
    }

    fn explain(rows: Option<f64>, cost: Option<f64>) -> Explain {
        let root = PlanNode {
            rows,
            cost,
            ..PlanNode::new("Seq Scan")
        };
        Explain::new(ExplainFormat::Text, String::new(), root, false)
    }

    #[test]
    fn a_plan_over_a_threshold_is_refused_and_a_missing_estimate_is_unchecked() {
        let big = explain(Some(50_000.0), Some(10.0));
        let by_rows = AgentLimits {
            max_estimated_rows: Some(1000),
            ..AgentLimits::default()
        };
        let Verdict::Over(why) = verdict(&big, &by_rows) else {
            panic!("expected a refusal");
        };
        assert!(why.contains("50000 rows") && why.contains("max_estimated_rows of 1000"));
        let by_cost = AgentLimits {
            max_plan_cost: Some(5),
            ..AgentLimits::default()
        };
        assert!(matches!(verdict(&big, &by_cost), Verdict::Over(_)));
        let both = AgentLimits {
            max_plan_cost: Some(100),
            max_estimated_rows: Some(100_000),
            ..AgentLimits::default()
        };
        assert_eq!(verdict(&big, &both), Verdict::Fine);
        // No estimate: not refused, the caller says it could not check.
        assert_eq!(verdict(&explain(None, None), &by_rows), Verdict::Unchecked);
        assert_eq!(
            verdict(&explain(Some(1.0), None), &both),
            Verdict::Unchecked
        );
        // A limit that is not set needs no figure.
        assert_eq!(
            verdict(
                &explain(Some(1.0), None),
                &AgentLimits {
                    max_estimated_rows: Some(5),
                    ..AgentLimits::default()
                }
            ),
            Verdict::Fine
        );
    }

    #[test]
    fn the_figure_of_a_step_stands_in_for_a_root_without_one() {
        let child = PlanNode {
            rows: Some(9000.0),
            ..PlanNode::new("Scan")
        };
        let root = PlanNode {
            children: vec![child],
            ..PlanNode::new("Aggregate")
        };
        assert_eq!(estimate(&root, |n| n.rows), Some(9000.0));
    }

    #[test]
    fn a_plan_does_not_carry_the_values_of_sensitive_columns() {
        let plan = "-> Seq Scan on users  (cost=1500, rows=20)\n     Filter: ((email = 'a@b.c') AND (id = 7))\n     Filter: (name = 'bob')\n     Filter: (ssn = 123456789)\n  -> Index Scan on users_email_idx  (cost=3, rows=1)";
        let masked = mask_plan(plan, &|w| {
            w == "email" || w == "ssn" || w == "users_email_idx"
        });
        assert!(
            !masked.contains("a@b.c") && !masked.contains("123456789"),
            "{masked}"
        );
        assert!(masked.contains("name = 'bob'"), "other columns stay");
        assert!(masked.contains("(cost=1500, rows=20)"), "step figures stay");
        assert!(masked.contains("(cost=3, rows=1)"));
        let doubled = mask_plan("     Filter: (email = 'it''s')", &|w| w == "email");
        assert!(!doubled.contains("it"), "{doubled}");
    }
}
