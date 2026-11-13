// qa4-ana2-1: "Create index..." from an explain suggestion drops the table's schema.
// Steps: PostgreSQL, a table qa4a2.items (20000 rows, not in the search path), editor on the
//   connection: SELECT * FROM qa4a2.items WHERE kind = 'k1', Explain analyze; the plan warns
//   that items is read in full; click "Create index...".
// Expected: the new editor holds CREATE INDEX ... ON "qa4a2"."items" (kind); (or the schema
//   is otherwise named), so running it indexes the table the plan read.
// Actual: CREATE INDEX items_kind_idx ON items (kind); which fails with "relation items does
//   not exist", or indexes another table called items in the search path.
// Code: crates/platypus-driver-pg/src/explain.rs:295 (FullScan.table = "Relation Name" only,
//   the plan's "Schema" is not used); apps/desktop/src/explain/indexSql.ts createIndexSql.
import assert from "node:assert/strict";
import { sh } from "../grid-helpers.mjs";
import { editorMenu, openEditor, saveConnections, setEditor, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  sh("pg", `DROP SCHEMA IF EXISTS qa4a2 CASCADE; CREATE SCHEMA qa4a2;
    CREATE TABLE qa4a2.items AS SELECT g AS id, 'k' || (g % 7) AS kind FROM generate_series(1, 20000) g;
    ANALYZE qa4a2.items;`);
}

const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;

export default async function (page) {
  try {
    await openEditor(page, "QA PG");
    await setEditor(page, "SELECT * FROM qa4a2.items WHERE kind = 'k1'");
    await editorMenu(page, "Explain analyze");
    await page.waitFor(`${explainTab}?.querySelector('.explain-suggestion button') != null`, 20_000);
    const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
    await page.eval(`${explainTab}.querySelector('.explain-suggestion button').click()`);
    await page.waitFor(`document.querySelectorAll('.query-tab').length === ${before + 1}`, 10_000);
    await page.waitFor(`/CREATE INDEX/.test(${queryTab}.querySelector('.cm-content').textContent)`, 10_000);
    const sql = await page.eval(`${queryTab}.querySelector('.cm-content').textContent`);
    assert.match(sql, /qa4a2/, `the statement must name the schema: ${sql}`);
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa4a2 CASCADE;");
  }
}
