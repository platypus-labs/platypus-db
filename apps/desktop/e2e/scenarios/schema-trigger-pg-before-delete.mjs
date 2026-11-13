// SCH4-05 (high). The trigger form on PostgreSQL writes `RETURN NEW;` for every row trigger that is not
// DELETE-only. For a BEFORE trigger that also fires on DELETE, NEW is NULL, so every DELETE is silently
// cancelled: the trigger runs, the row stays.
// Steps: qa3s_q4.t (id, v) with two rows; Create trigger form (here its backend, object_form_sql, the
// same SQL the form previews) BEFORE INSERT OR DELETE, FOR EACH ROW, body
// INSERT INTO audit VALUES (TG_OP); run it; DELETE FROM t WHERE id = 1.
// Expected: the row is deleted (the generated function returns OLD for DELETE, e.g. COALESCE(NEW, OLD)).
// Actual: "DELETE 0"-like no-op: both rows are still there, the audit table has the DELETE.
// Code: crates/platypus-schema/src/objects.rs postgres_trigger (`events == [Event::Delete]` is the only
// case that returns OLD).
import assert from "node:assert/strict";
import { Q4, ENGINES, saveEngines, seed } from "../schema-helpers.mjs";
import { invoke, needsEngines, psql } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  seed(dir, "pg", `CREATE TABLE S.t (id int PRIMARY KEY, v text); CREATE TABLE S.audit (op text); INSERT INTO S.t VALUES (1, 'a'), (2, 'b');`);
  saveEngines(dir, ["pg"]);
}

export default async function (page) {
  try {
    const made = await invoke(page, "object_form_sql", {
      driver: "postgres",
      schema: Q4,
      form: {
        kind: "trigger",
        name: "tg",
        table: "t",
        timing: "before",
        events: ["insert", "delete"],
        for_each_row: true,
        when: null,
        body: `INSERT INTO ${Q4}.audit VALUES (TG_OP)`,
        replace: false,
      },
    });
    assert.equal(made.err, undefined, made.err);
    psql(made.ok.map((s) => `${s};`).join("\n"));
    psql(`DELETE FROM ${Q4}.t WHERE id = 1`);
    assert.equal(psql(`SELECT count(*) FROM ${Q4}.t`), "1", `the DELETE was cancelled by the trigger:\n${made.ok.join(";\n")}`);
  } finally {
    ENGINES.pg.drop(Q4);
  }
}

