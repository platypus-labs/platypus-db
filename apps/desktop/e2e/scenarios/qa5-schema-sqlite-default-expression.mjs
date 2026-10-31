// QA5-S1 (high). The SQLite table editor writes a column default as the database reports it,
// and SQLite wants a default that is not a literal in parentheses: DEFAULT (datetime('now')).
// So any table with such a default (the most common kind of SQLite timestamp column) cannot be
// redefined (the table is created again and the script fails with a syntax error), and a new
// column or table cannot be given one by typing datetime('now') in the Default cell.
// Steps: t (id INTEGER PRIMARY KEY, created TEXT DEFAULT (datetime('now')), n TEXT) with a
// row; table editor > n NOT NULL > Save (table created again). Then Create table with a
// column whose default is datetime('now') (and another with 1+2).
// Expected: both scripts run; the row is kept; the default is still there afterwards.
// Actual: "near "(": syntax error": CREATE TABLE main.t (... created TEXT DEFAULT datetime('now') ...).
// Code: crates/platypus-schema/src/ddl.rs (column definition, `Some(default) => format!(" DEFAULT {default}")`:
// no parentheses for an expression on SQLite; the SQLite driver reports dflt_value without them).
import assert from "node:assert/strict";
import { lab, saveEngines, seed } from "../qa4-schema.mjs";
import { needsEngines, invoke } from "../qa3-schema.mjs";

export const skip = needsEngines("sqlite");

const col = (name, data_type, o = {}) => ({ original: null, name, data_type, nullable: true, default: null, primary_key: false, comment: null, extra: null, ...o });

export function setup({ dir }) {
  seed(dir, "sqlite", `CREATE TABLE t (id INTEGER PRIMARY KEY, created TEXT DEFAULT (datetime('now')), n TEXT); INSERT INTO t (id, n) VALUES (1, 'a');`);
  saveEngines(dir, ["sqlite"]);
}

export default async function (page, { dir }) {
  const problems = [];
  try {
    const l = await lab(page, dir, "sqlite");
    const r = await l.edit("t", (d) => {
      d.columns.find((c) => c.name === "n").nullable = false;
    });
    if (r.error) problems.push(`redefining a table with DEFAULT (datetime('now')): ${r.error}\n${r.sql}`);
    const created = await l.script(null, {
      name: "nt",
      columns: [col("id", "INTEGER", { nullable: false, primary_key: true }), col("c", "TEXT", { default: "datetime('now')" }), col("d", "INT", { default: "1+2" })],
      primary_key_name: null, indexes: [], foreign_keys: [], constraints: [], comment: null, options: null,
    });
    const run = await invoke(page, "apply_table_script", { sessionId: l.sid, script: created.script });
    if (run.err) problems.push(`a new table with default datetime('now'): ${run.err}\n${created.sql}`);
  } catch (e) {
    problems.push(String(e.message));
  }
  assert.deepEqual(problems, []);
}
