// BUG ANA-09 (medium): in the SQL field above a result, a condition that starts with a parenthesis
// is run as a whole query and fails: `(grp = 1 OR grp = 2) AND name = 'a'` gives "syntax error at
// or near …". The field reads "(" as the start of a query (`(SELECT …) UNION …`).
// BUG ANA-10 (low, same function): a condition on a column named like a word that starts a query
// (values, table, show, from, describe, pivot…) is also run as a query: `values = 3` fails.
// Steps: run `SELECT id, grp, name, "values" FROM items`; type each condition in the field, Enter.
// Expected: the rows that match (2 and 1). Actual: a syntax error under the field.
// Suspected: apps/desktop/src/analysis/sqlField.ts `QUERY_START` / `resultQuery`: only "(" followed
// by SELECT/WITH/VALUES/FROM starts a query; a keyword followed by an operator is a condition.
import assert from "node:assert/strict";
import { gridTsv, inField, liteConnection, newEditor, runQuery, status, visibleTab } from "../analysis-helpers.mjs";

const NAME = "QA ana paren";

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0111",
    `CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT, "values" INTEGER);
     INSERT INTO items VALUES (1, 1, 'a', 3), (2, 2, 'a', 4), (3, 3, 'a', 5), (4, 1, 'b', 6);`,
  );
}

async function back(page) {
  await page.eval(`${visibleTab}.querySelector('.result-sql .cm-content').focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${visibleTab}.querySelector('.result-sql-mark') == null`);
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, `SELECT id, grp, name, "values" FROM items ORDER BY id`);
  assert.equal(await page.eval(status), "4 rows");

  // A condition alone works.
  let run = await inField(page, "grp = 1");
  assert.equal(run.error, "");
  assert.match(run.mark, /^SQL view · 2 rows/);
  await back(page);

  run = await inField(page, "(grp = 1 OR grp = 2) AND name = 'a'");
  assert.equal(run.error, "", "a condition in parentheses is a WHERE");
  assert.equal(await gridTsv(page), "id\tgrp\tname\tvalues\n1\t1\ta\t3\n2\t2\ta\t4");
  await back(page);

  run = await inField(page, "values = 3");
  assert.equal(run.error, "", "a column named values is a condition");
  assert.match(run.mark, /^SQL view · 1 row/);
}
