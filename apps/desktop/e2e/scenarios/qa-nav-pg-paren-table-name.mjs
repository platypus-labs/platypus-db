// Bug NAV: on PostgreSQL, a table whose name contains `(` gets unquoted, broken SQL from the
// navigator: Generate SELECT/INSERT/UPDATE/DELETE write `FROM qa_nav_paren.qa_nav_t(1)`, Copy
// qualified name copies `qa_nav_paren.qa_nav_t(1)`, Drop… and Truncate… preview the same
// unquoted name, and Rename… starts from `qa_nav_t`.
//
// Cause: the text after `(` is taken for a routine's argument list whatever the object's kind:
// `Dialect::object` in crates/platypus-sql/src/generate.rs, `qualified()` in
// apps/desktop/src/objects/actions.ts and the Rename default in the same file.
// Generate DDL and Open data quote the name correctly.
//
// Steps: right-click the table `qa_nav_t(1)` › Generate SELECT (and Copy qualified name).
// Expected: `FROM qa_nav_paren."qa_nav_t(1)"`. Actual: `FROM qa_nav_paren.qa_nav_t(1)`.
import assert from "node:assert/strict";
import { DBS, connectTo, editorText, expand, menu, needs, row, saveConnections, visibleTab } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = "qa_nav_paren";
const TABLE = "qa_nav_t(1)";

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;
    CREATE SCHEMA ${SCHEMA};
    CREATE TABLE ${SCHEMA}."${TABLE}" (id int PRIMARY KEY, v text);`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0012"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, SCHEMA, TABLE);

    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }; window.__copied = undefined`);
    await menu(page, TABLE, "Copy qualified name");
    await page.waitFor(`window.__copied !== undefined`);
    const copied = await page.eval(`window.__copied`);

    await menu(page, TABLE, "Generate SELECT");
    await page.waitFor(`/^SELECT/.test(${editorText})`, 10_000);
    const select = await page.eval(editorText);

    await menu(page, TABLE, "Drop…");
    await page.waitFor(`/DROP TABLE/.test(document.querySelector('.modal.sql-action .sql-preview')?.textContent ?? '')`, 10_000);
    const drop = await page.eval(`document.querySelector('.modal.sql-action .sql-preview').textContent`);
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Cancel').click()`);

    assert.equal(copied, `${SCHEMA}."${TABLE}"`, "Copy qualified name");
    assert.match(select, new RegExp(`FROM ${SCHEMA}\\."qa_nav_t\\(1\\)"`), `Generate SELECT: ${select}`);
    assert.match(drop, new RegExp(`DROP TABLE ${SCHEMA}\\."qa_nav_t\\(1\\)"`), `Drop: ${drop}`);
    assert.ok(await page.eval(`${row(TABLE)} !== undefined && ${visibleTab} !== undefined`));
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;`);
  }
}
