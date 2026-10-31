// qa4-ana2-8: "Create index..." from an explain suggestion indexes the bare column of a
// condition on an expression, an index PostgreSQL cannot use for that condition.
// Steps: PostgreSQL, table qa4a2e.people (20000 rows); editor on the connection:
//   SELECT * FROM qa4a2e.people WHERE lower(email) = 'x1@example.com', Explain analyze; click
//   "Create index...".
// Expected: an index on the expression (lower(email)), or no "Create index..." button for a
//   condition the app cannot index.
// Actual: CREATE INDEX people_email_idx ON people (email); the plain index on email: running it
//   changes nothing, the same Seq Scan stays (and with the schema missing, see qa4-ana2-1).
// Code: apps/desktop/src/explain/indexSql.ts filterColumns (reads `email` before `)` and `=`,
//   drops the function around it) and ExplainTab.tsx (button for every non-empty column list).
import assert from "node:assert/strict";
import { sh } from "../qa-grid.mjs";
import { editorMenu, openEditor, saveConnections, setEditor, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  sh("pg", `DROP SCHEMA IF EXISTS qa4a2e CASCADE; CREATE SCHEMA qa4a2e;
    CREATE TABLE qa4a2e.people AS SELECT g AS id, 'X' || g || '@Example.com' AS email FROM generate_series(1, 20000) g;
    ANALYZE qa4a2e.people;`);
}

const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;

export default async function (page) {
  try {
    await openEditor(page, "QA PG");
    await setEditor(page, "SELECT * FROM qa4a2e.people WHERE lower(email) = 'x1@example.com'");
    await editorMenu(page, "Explain analyze");
    await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 20_000);
    await page.sleep(500);
    const button = await page.eval(`${explainTab}.querySelector('.explain-suggestion button')?.textContent ?? null`);
    if (button === null) return; // no offer for what cannot be indexed by column: fine
    const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
    await page.eval(`${explainTab}.querySelector('.explain-suggestion button').click()`);
    await page.waitFor(`document.querySelectorAll('.query-tab').length === ${before + 1}`, 10_000);
    await page.waitFor(`/CREATE INDEX/.test(${queryTab}.querySelector('.cm-content').textContent)`, 10_000);
    const sql = await page.eval(`${queryTab}.querySelector('.cm-content').textContent`);
    assert.match(sql, /lower/i, `the index must be on lower(email): ${sql}`);
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa4a2e CASCADE;");
  }
}
