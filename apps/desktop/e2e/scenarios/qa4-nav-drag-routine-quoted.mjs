// qa4-nav-1 (medium). A routine dragged from the navigator into the SQL editor is dropped with
// its whole row label quoted as one identifier.
// Steps: PostgreSQL, schema qa4_nav_dr with function f_one(x int); open a query tab (Generate SELECT
//   on its table), drag the row `f_one(x integer)` from the navigator into the editor text.
// Expected: a name that works in SQL: `qa4_nav_dr.f_one` (Copy qualified name gives
//   `qa4_nav_dr.f_one(x integer)`, the call syntax is the person's to write).
// Actual: `qa4_nav_dr."f_one(x integer)"`, an identifier no object has (the arguments are
//   part of the quoted name), so the query fails with "function ... does not exist"/syntax error.
// Code: apps/desktop/src/components/Navigator.tsx objectRows(): for a non-relation
//   `sql: { name: target }`, and `target = qualifiedName(driver, schema, object.name)` where
//   `object.name` of a PostgreSQL routine is the label with its signature.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { DBS, connectTo, editorText, expand, menu, needs, row, saveConnections, visibleTab } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS qa4_nav_dr CASCADE; CREATE SCHEMA qa4_nav_dr;
    CREATE TABLE qa4_nav_dr.t (id int);
    CREATE FUNCTION qa4_nav_dr.f_one(x int) RETURNS int LANGUAGE sql AS 'select 1';`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a4001"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, "qa4_nav_dr", "t");
    await page.eval(`[...document.querySelectorAll('.tree-row')].filter(r => r.getAttribute('aria-expanded') === 'false' && r.querySelector('.tree-label')?.textContent === 'Functions').forEach(r => r.querySelector('.twisty').click())`);
    await page.waitFor(`${row("f_one(x integer)")} !== undefined`, 10_000);
    await menu(page, "t", "Generate SELECT");
    await page.waitFor(`${editorText}.startsWith('SELECT')`, 10_000);
    const content = `${visibleTab}.querySelector('.cm-content')`;
    await page.eval(`${content}.focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.type("SELECT ");
    await page.waitFor(`${editorText} === 'SELECT '`);
    const from = await page.rectOf(`${row("f_one(x integer)")}.querySelector('.tree-label')`);
    const to = await page.rectOf(content);
    await page.drag(from.x + from.w / 2, from.y + from.h / 2, to.x + 200, to.y + 8);
    await page.waitFor(`${editorText}.includes('qa4_nav_dr')`, 5_000);
    const text = await page.eval(editorText);
    assert.doesNotMatch(text, /"f_one\(/, `the signature went inside the quotes: ${text}`);
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS qa4_nav_dr CASCADE;`);
  }
}
