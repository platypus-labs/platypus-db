//! Query plans: the `explain` command for an operation, and its answer as a tree of
//! [`PlanNode`]s. `explain` never applies a write, whatever its verbosity.
//!
//! The plan names no index to suggest: the app's suggestions are `CREATE INDEX` statements.

use mongodb::bson::{Bson, Document, doc};
use platypus_core::PlanNode;

use crate::shell::{CollectionOp, Find};

/// The server command that `op` on collection `name` is, as `explain` takes it; `None` for what
/// the server has no plan of.
#[must_use]
pub fn command_of(name: &str, op: CollectionOp) -> Option<Document> {
    Some(match op {
        CollectionOp::Find(find) => find_command(name, find),
        CollectionOp::Aggregate { pipeline, options } => {
            let mut command = doc! { "aggregate": name, "pipeline": pipeline, "cursor": {} };
            command.extend(options.into_iter().filter(|(k, _)| k != "cursor"));
            command
        }
        CollectionOp::CountDocuments {
            filter,
            skip,
            limit,
            options,
        } => {
            let mut command = doc! { "count": name, "query": filter };
            optional(&mut command, "skip", skip);
            optional(&mut command, "limit", limit);
            command.extend(options);
            command
        }
        CollectionOp::Distinct {
            field,
            filter,
            options,
        } => {
            let mut command = doc! { "distinct": name, "key": field, "query": filter };
            command.extend(options);
            command
        }
        CollectionOp::Update {
            filter,
            update,
            many,
            upsert,
            options,
        } => update_command(name, filter, update, many, upsert, options),
        CollectionOp::Replace {
            filter,
            replacement,
            upsert,
            options,
        } => update_command(
            name,
            filter,
            Bson::Document(replacement),
            false,
            upsert,
            options,
        ),
        CollectionOp::Delete {
            filter,
            many,
            options,
        } => {
            let mut statement = doc! { "q": filter, "limit": i32::from(!many) };
            statement.extend(options);
            doc! { "delete": name, "deletes": [statement] }
        }
        CollectionOp::FindOneAndUpdate {
            filter,
            update,
            options,
        } => find_and_modify(name, filter, doc! { "update": update }, options),
        CollectionOp::FindOneAndReplace {
            filter,
            replacement,
            options,
        } => find_and_modify(name, filter, doc! { "update": replacement }, options),
        CollectionOp::FindOneAndDelete { filter, options } => {
            find_and_modify(name, filter, doc! { "remove": true }, options)
        }
        _ => return None,
    })
}

fn optional(command: &mut Document, key: &str, value: Option<u64>) {
    if let Some(n) = value.and_then(|n| i64::try_from(n).ok()) {
        command.insert(key, n);
    }
}

fn find_command(name: &str, find: Find) -> Document {
    let mut command = doc! { "find": name, "filter": find.filter };
    if let Some(projection) = find.projection {
        command.insert("projection", projection);
    }
    if let Some(sort) = find.sort {
        command.insert("sort", sort);
    }
    optional(&mut command, "skip", find.skip);
    if let Some(limit) = find.limit {
        command.insert("limit", limit);
    }
    command.extend(find.options);
    command
}

fn update_command(
    name: &str,
    filter: Document,
    update: Bson,
    many: bool,
    upsert: bool,
    options: Document,
) -> Document {
    let mut statement = doc! { "q": filter, "u": update, "multi": many, "upsert": upsert };
    statement.extend(options);
    doc! { "update": name, "updates": [statement] }
}

fn find_and_modify(name: &str, filter: Document, what: Document, options: Document) -> Document {
    let mut command = doc! { "findAndModify": name, "query": filter };
    command.extend(what);
    for (key, value) in options {
        match key.as_str() {
            "projection" => command.insert("fields", value),
            "returnDocument" => command.insert("new", value.as_str() == Some("after")),
            _ => command.insert(key, value),
        };
    }
    command
}

/// The plan an `explain` answered, as a tree. An aggregation that the server splits in stages
/// reads from the last to the first, each stage feeding the next.
#[must_use]
pub fn plan_tree(reply: &Document) -> PlanNode {
    if let Ok(stages) = reply.get_array("stages") {
        return stages_tree(stages);
    }
    single_plan(reply).unwrap_or_else(|| PlanNode::new("Plan"))
}

fn stages_tree(stages: &[Bson]) -> PlanNode {
    let mut below: Option<PlanNode> = None;
    for stage in stages.iter().filter_map(Bson::as_document) {
        let mut node = stage_of_pipeline(stage);
        if let Some(child) = below.take() {
            node.children.push(child);
        }
        below = Some(node);
    }
    below.unwrap_or_else(|| PlanNode::new("Aggregation"))
}

/// One entry of `stages`: `{ $cursor: { queryPlanner, … } }`, or `{ $group: {…}, nReturned, … }`.
fn stage_of_pipeline(stage: &Document) -> PlanNode {
    if let Ok(cursor) = stage.get_document("$cursor")
        && let Some(plan) = single_plan(cursor)
    {
        return plan;
    }
    let Some((name, spec)) = stage.iter().find(|(key, _)| key.starts_with('$')) else {
        return PlanNode::new("Stage");
    };
    let mut node = PlanNode::new(name.clone());
    node.detail = Some(relaxed(spec));
    node.actual_rows = number(stage, "nReturned");
    node.actual_ms = number(stage, "executionTimeMillisEstimate");
    node
}

/// The plan of `{ queryPlanner, executionStats }`: what ran when there are statistics, what was
/// planned otherwise.
fn single_plan(reply: &Document) -> Option<PlanNode> {
    if let Some(stages) = reply
        .get_document("executionStats")
        .ok()
        .and_then(|stats| stats.get_document("executionStages").ok())
    {
        return Some(node_of(stages));
    }
    let planner = reply.get_document("queryPlanner").ok()?;
    let winning = planner.get_document("winningPlan").ok()?;
    // Newer servers wrap it: `{ queryPlan: {…}, slotBasedPlan: {…} }`.
    Some(node_of(
        winning.get_document("queryPlan").unwrap_or(winning),
    ))
}

fn node_of(stage: &Document) -> PlanNode {
    let operation = stage.get_str("stage").unwrap_or("Stage");
    let mut node = PlanNode::new(operation);
    node.detail = stage
        .get_str("indexName")
        .ok()
        .map(str::to_owned)
        .or_else(|| stage.get("filter").map(relaxed));
    node.actual_rows = number(stage, "nReturned");
    node.actual_ms = number(stage, "executionTimeMillisEstimate");
    for (label, key) in [
        ("Documents examined", "docsExamined"),
        ("Keys examined", "keysExamined"),
        ("Works", "works"),
        ("Direction", "direction"),
        ("Key pattern", "keyPattern"),
        ("Index bounds", "indexBounds"),
        ("Filter", "filter"),
        ("Sort", "sortPattern"),
        ("Limit", "limitAmount"),
        ("Skip", "skipAmount"),
    ] {
        if let Some(value) = stage.get(key) {
            node.properties.push((label.to_owned(), relaxed(value)));
        }
    }
    if let Ok(child) = stage.get_document("inputStage") {
        node.children.push(node_of(child));
    }
    if let Ok(children) = stage.get_array("inputStages") {
        node.children
            .extend(children.iter().filter_map(Bson::as_document).map(node_of));
    }
    node
}

/// A value as a person reads it: a string bare, anything else as JSON.
fn relaxed(value: &Bson) -> String {
    match value {
        Bson::String(text) => text.clone(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}

fn number(stage: &Document, key: &str) -> Option<f64> {
    match stage.get(key)? {
        Bson::Int32(n) => Some(f64::from(*n)),
        #[allow(clippy::cast_precision_loss)]
        Bson::Int64(n) => Some(*n as f64),
        Bson::Double(n) => Some(*n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{Command, parse};

    fn command(src: &str) -> Document {
        let Ok(Command::Collection { op, .. }) = parse(src) else {
            panic!("not a collection statement: {src}");
        };
        command_of("c", op).unwrap()
    }

    #[test]
    fn builds_the_commands_with_their_options() {
        let find = command(
            "db.c.find({ a: 1 }, { b: 1 }).sort({ a: -1 }).limit(2).skip(3).hint('a_1').collation({ locale: 'it' })",
        );
        assert_eq!(
            find,
            doc! {
                "find": "c",
                "filter": { "a": 1 },
                "projection": { "b": 1 },
                "sort": { "a": -1 },
                "skip": 3_i64,
                "limit": 2_i64,
                "hint": "a_1",
                "collation": { "locale": "it" },
            }
        );
        let update = command(
            "db.c.updateMany({ a: 1 }, { $set: { 'x.$[e]': 1 } }, { arrayFilters: [{ e: 2 }], upsert: true })",
        );
        let statement = update.get_array("updates").unwrap()[0]
            .as_document()
            .unwrap();
        assert_eq!(statement.get_bool("multi"), Ok(true));
        assert_eq!(statement.get_bool("upsert"), Ok(true));
        assert!(statement.get_array("arrayFilters").is_ok());
        let delete = command("db.c.deleteOne({ a: 1 })");
        let statement = delete.get_array("deletes").unwrap()[0]
            .as_document()
            .unwrap();
        assert_eq!(statement.get_i32("limit"), Ok(1));
        let many = command("db.c.deleteMany({ a: 1 })");
        let statement = many.get_array("deletes").unwrap()[0].as_document().unwrap();
        assert_eq!(statement.get_i32("limit"), Ok(0));
        let modify = command(
            "db.c.findOneAndUpdate({ a: 1 }, { $set: { b: 1 } }, { returnDocument: 'after', projection: { b: 1 } })",
        );
        assert_eq!(modify.get_bool("new"), Ok(true));
        assert!(modify.get_document("fields").is_ok());
        assert!(modify.get("projection").is_none());
    }

    #[test]
    fn reads_a_planned_tree() {
        let reply = doc! {
            "queryPlanner": {
                "winningPlan": {
                    "stage": "LIMIT",
                    "limitAmount": 5,
                    "inputStage": {
                        "stage": "FETCH",
                        "filter": { "b": { "$eq": 1 } },
                        "inputStage": { "stage": "IXSCAN", "indexName": "a_1", "direction": "forward" },
                    },
                },
            },
        };
        let root = plan_tree(&reply);
        assert_eq!(root.operation, "LIMIT");
        assert_eq!(root.children[0].operation, "FETCH");
        assert!(root.children[0].detail.as_deref().unwrap().contains("$eq"));
        let index = &root.children[0].children[0];
        assert_eq!(index.operation, "IXSCAN");
        assert_eq!(index.detail.as_deref(), Some("a_1"));
        assert!(root.actual_rows.is_none());
    }

    #[test]
    fn reads_what_ran_and_the_newer_wrapper() {
        let ran = doc! {
            "queryPlanner": { "winningPlan": { "stage": "IGNORED" } },
            "executionStats": {
                "executionStages": {
                    "stage": "COLLSCAN",
                    "nReturned": 3,
                    "executionTimeMillisEstimate": 7,
                    "docsExamined": 10,
                },
            },
        };
        let root = plan_tree(&ran);
        assert_eq!(root.operation, "COLLSCAN");
        assert_eq!(root.actual_rows, Some(3.0));
        assert_eq!(root.actual_ms, Some(7.0));
        assert!(
            root.properties
                .contains(&("Documents examined".to_owned(), "10".to_owned()))
        );
        // The app's suggestions are SQL: none from here.
        assert!(root.full_scan.is_none());

        let wrapped = doc! {
            "queryPlanner": { "winningPlan": { "queryPlan": { "stage": "EOF" }, "slotBasedPlan": { "stages": "x" } } },
        };
        assert_eq!(plan_tree(&wrapped).operation, "EOF");
    }

    #[test]
    fn reads_the_stages_of_an_aggregation_last_first() {
        let reply = doc! {
            "stages": [
                { "$cursor": { "queryPlanner": { "winningPlan": { "stage": "COLLSCAN" } } } },
                { "$group": { "_id": "$a" }, "nReturned": 2 },
                { "$sort": { "sortKey": { "_id": 1 } } },
            ],
        };
        let root = plan_tree(&reply);
        assert_eq!(root.operation, "$sort");
        assert_eq!(root.children[0].operation, "$group");
        assert_eq!(root.children[0].actual_rows, Some(2.0));
        assert_eq!(root.children[0].children[0].operation, "COLLSCAN");
    }

    #[test]
    fn an_answer_without_a_plan_is_still_a_tree() {
        assert_eq!(plan_tree(&doc! { "ok": 1 }).operation, "Plan");
        assert_eq!(plan_tree(&doc! { "ok": 1 }).children.len(), 0);
    }
}
