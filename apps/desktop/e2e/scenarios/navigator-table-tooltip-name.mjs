// Bug NAV (low, UX): a table whose name is cut with an ellipsis in the navigator cannot be read
// in full: on PostgreSQL its tooltip shows only the size (`0 B`, or `~1,234 rows · 3.4 MB`),
// on SQLite (no size, no comment) there is no tooltip at all. Other rows fall back to
// `label  detail`, but a table's title is its size and comment only, never its name.
//
// Cause: `objectRows` in apps/desktop/src/components/Navigator.tsx sets
// `title: [annotation, sizeDetail].join("\n")` for relations, without the label.
//
// Steps: a table with a 60-character name; hover it in the navigator (default width).
// Expected: the tooltip holds the full name. Actual: none (SQLite) or `0 B` (PostgreSQL).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { row, saveConnections } from "../navigator-helpers.mjs";

const LONG = "qa_nav_a_table_with_a_rather_long_name_that_does_not_fit_here";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "long.db"));
  db.exec(`CREATE TABLE ${LONG} (id INTEGER);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0045"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "long.db")}'
save_password = false
`);
}

export default async function (page) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row(LONG)} !== undefined`, 15_000);
  const label = `${row(LONG)}.querySelector('.tree-label')`;
  assert.ok(await page.eval(`${label}.scrollWidth > ${label}.clientWidth`), "the name is cut");
  const title = await page.eval(`${row(LONG)}.title`);
  assert.ok(title.includes(LONG), `tooltip: ${JSON.stringify(title)}`);
}
