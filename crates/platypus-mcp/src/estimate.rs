//! `estimate_change`: what the confirmation dialog would show the user for a statement,
//! without running it: the rows the database expects to change, its plan and the checks that
//! would ask or refuse. The plan comes from `EXPLAIN` without `ANALYZE`, so nothing runs.

use platypus_core::{DriverKind, PlanNode};
use platypus_sql::Dialect;
use platypus_store::SavedConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::access::{self, UnfilteredRule};
use crate::agent::{Agent, PLAN_BYTES};
use crate::error::{AgentError, Result};
use crate::guard::{Asked, clamp, has_plans};
use crate::plan::PlanResult;
use crate::query::{self, Limits};

/// The arguments of `estimate_change`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EstimateArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// One SQL statement (or command): the change to estimate. It is never run.
    pub sql: String,
}

/// The rows a plan expects the statement to change. A change's root (PostgreSQL's
/// `ModifyTable`) estimates no rows: the rows it reads do.
pub(crate) fn expected_rows(root: &PlanNode) -> Option<f64> {
    std::iter::once(root)
        .chain(root.children.first())
        .find_map(|n| n.rows.filter(|r| *r > 0.0))
        .or(root.rows)
}

/// `sql` as `run_query` checks it on `saved`.
fn check<'a>(sql: &'a str, saved: &SavedConnection, unfiltered: UnfilteredRule) -> Result<&'a str> {
    let access = saved.effective_agent_access();
    let name = &saved.name;
    match saved.driver {
        DriverKind::Mongo => access::check_mongo(sql, access, unfiltered, name),
        DriverKind::Redis => access::check_redis(sql, access, unfiltered, name),
        DriverKind::Elasticsearch => access::check_search(sql, access, unfiltered, name),
        DriverKind::Cassandra => access::check_cql(sql, access, unfiltered, name),
        driver => access::check(sql, Dialect::from(driver), access, unfiltered, name),
    }
}

/// What the Safety settings say about `statement`, one line each.
fn safety_checks(statement: &str, driver: DriverKind, rows: Option<f64>) -> Vec<String> {
    if !has_plans(driver) {
        return Vec::new();
    }
    let dialect = Dialect::from(driver);
    let mut checks = Vec::new();
    for every in platypus_sql::analyze(statement, dialect).unfiltered {
        let table = match &every.table.schema {
            Some(schema) => format!("{schema}.{}", every.table.name),
            None => every.table.name.clone(),
        };
        let how_many = rows.map_or(String::new(), |r| format!(" (about {r:.0} rows)"));
        let limit = if every.limited {
            ", up to its TOP limit"
        } else {
            ""
        };
        checks.push(format!(
            "{} without WHERE changes every row of `{table}`{limit}{how_many}: the Safety setting \
             on it applies",
            every.verb
        ));
    }
    if let Some(word) = access::outside_effect(statement, dialect) {
        checks.push(format!(
            "`{word}` acts outside the transaction or on the server's files"
        ));
    }
    checks
}

/// Why no estimate could be made, as the answer says it.
fn no_estimate(why: &str) -> Value {
    json!({ "estimated_rows": null, "plan": null, "estimate_note": why })
}

impl Agent {
    /// What the confirmation dialog would show for `args.sql` on the connection, and whether
    /// the statement would run for an agent at all; nothing runs, not even the statement's
    /// functions: only the planner is asked.
    pub async fn estimate_change(&self, args: &EstimateArgs) -> Result<Value> {
        let (store, saved) = self.find(&args.connection)?;
        self.windows.count(&saved)?;
        let rule = self.unfiltered_rule(&store);
        let statement = match check(&args.sql, &saved, rule) {
            Ok(statement) => statement,
            // A statement the access refuses is an answer: the agent corrects it before asking.
            Err(AgentError::Denied(reason)) => {
                return Ok(json!({
                    "connection": saved.name,
                    "outcome": "refused",
                    "would_run": false,
                    "reason": reason,
                }));
            }
            Err(e) => return Err(e),
        };
        if !access::asks_first(statement, saved.driver) {
            return Ok(json!({
                "connection": saved.name,
                "outcome": "not_a_change",
                "is_change": false,
                "note": "this statement changes nothing and needs no confirmation: run_query \
                         runs it as it is",
            }));
        }
        let mut answer = self.estimate_plan(&store, &saved, statement).await?;
        let rows = answer["estimated_rows"].as_f64();
        let asks = self.confirms_changes();
        let app_open = self.link().is_some_and(|link| link.reachable());
        answer["connection"] = json!(saved.name);
        answer["outcome"] = json!("estimated");
        answer["is_change"] = json!(true);
        answer["agent_access"] = json!(saved.effective_agent_access());
        answer["safety_checks"] = json!(safety_checks(statement, saved.driver, rows));
        answer["asks_the_user"] = json!(asks);
        answer["app_open"] = json!(app_open);
        answer["would_run"] = json!(!asks || app_open);
        if asks && !app_open {
            answer["reason"] = json!(
                "PlatypusDB asks the user before an agent changes data, and the app is not open \
                 to ask: the change would be refused"
            );
        }
        Ok(answer)
    }

    /// `{estimated_rows, plan}` of `statement` from the database's plan, or why there is none;
    /// the plan has the values of sensitive columns masked.
    async fn estimate_plan(
        &self,
        store: &platypus_store::ConnectionStore,
        saved: &SavedConnection,
        statement: &str,
    ) -> Result<Value> {
        if !has_plans(saved.driver) {
            return Ok(no_estimate(&format!(
                "{} has no query plans here: there is no estimate",
                saved.driver.name()
            )));
        }
        let (limits, _) = clamp(
            Limits::new(None, None, None)?,
            &Asked::default(),
            &saved.agent_limits,
        );
        let lease = self.lease(store, saved).await?;
        if !lease.conn().capabilities().explain {
            return Ok(no_estimate(&format!(
                "`{}` has no query plans in PlatypusDB: there is no estimate",
                saved.name
            )));
        }
        // Without analyze: the planner reads the statement, the database does not run it.
        let work = lease.conn().explain(statement, false);
        let planned = query::guarded(
            lease.conn(),
            None,
            limits.timeout,
            std::future::pending(),
            work,
        )
        .await;
        let explain = match lease.finish(planned, limits.timeout) {
            Ok(explain) => explain,
            Err(e) => {
                let e = self.mask_statement_error(saved, e);
                return Ok(no_estimate(&format!("the database gave no plan: {e}")));
            }
        };
        let rows = expected_rows(&explain.root);
        let plan = PlanResult {
            explain,
            elapsed: std::time::Duration::ZERO,
            max_bytes: PLAN_BYTES.min(limits.max_bytes),
        }
        .text();
        let plan = self.mask_plan_text(saved, &plan)?;
        Ok(json!({ "estimated_rows": rows, "plan": plan }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(rows: Option<f64>, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            operation: "x".to_owned(),
            detail: None,
            cost: None,
            rows,
            actual_rows: None,
            actual_ms: None,
            loops: None,
            self_cost: None,
            self_ms: None,
            full_scan: None,
            properties: Vec::new(),
            children,
        }
    }

    #[test]
    fn a_change_reads_its_rows_from_the_step_below_the_root() {
        let plan = node(Some(0.0), vec![node(Some(42.0), Vec::new())]);
        assert_eq!(expected_rows(&plan), Some(42.0));
        assert_eq!(expected_rows(&node(Some(7.0), Vec::new())), Some(7.0));
        assert_eq!(expected_rows(&node(None, Vec::new())), None);
    }

    #[test]
    fn every_row_changes_are_named_with_their_table_and_rows() {
        let checks = safety_checks("DELETE FROM main.t", DriverKind::Sqlite, Some(120.0));
        assert_eq!(checks.len(), 1, "{checks:?}");
        assert!(checks[0].contains("DELETE") && checks[0].contains("`main.t`"));
        assert!(checks[0].contains("about 120 rows"), "{checks:?}");
        let filtered = safety_checks("DELETE FROM t WHERE id = 1", DriverKind::Sqlite, None);
        let mongo = safety_checks("db.t.deleteMany({})", DriverKind::Mongo, None);
        assert!(
            filtered.is_empty() && mongo.is_empty(),
            "{filtered:?} {mongo:?}"
        );
    }

    #[test]
    fn a_plan_less_driver_says_so() {
        let note = no_estimate("no plan");
        assert!(note["estimated_rows"].is_null() && note["estimate_note"] == "no plan");
    }
}
