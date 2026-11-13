// SCH-06 (medium). On PostgreSQL, Check schema… keeps reporting "Foreign key without index" for
// a foreign key on a column whose name needs quotes (a space, capitals, a reserved word) even
// once the index exists: the fix it proposes runs, and the finding is still there, so it can
// never be cleared. SQLite, SQL Server and Oracle see the index.
// Steps: qa3s_src.customers(id PK) and "Order Lines"("Customer Id" → customers); Check schema…
// > Open 1 fix in editor > run; Check schema… again.
// Expected: "No problems found." after the fix. Actual: the same finding again.
// Suspected: the index columns of the PostgreSQL driver come quoted ("Customer Id", from
// pg_get_indexdef) while the foreign key's do not, so crates/platypus-schema/src/lint.rs:99
// (starts_with_set) never matches them; see crates/platypus-driver-pg/src/meta.rs indexes.
import assert from "node:assert/strict";
import { ENGINES, SRC, buttonIn, lintDialog, needsEngines, openPath, rowMenu, runEditorScript, saveEngines, queryText } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.customers (id int PRIMARY KEY);
CREATE TABLE S."Order Lines" (id int PRIMARY KEY, "Customer Id" int REFERENCES S.customers(id), "user" int REFERENCES S.customers(id));`);
  saveEngines(dir, ["pg"]);
}

const findings = `[...${lintDialog}.querySelectorAll('.lint-findings li')].map(li => li.textContent)`;

export default async function (page) {
  try {
    await openPath(page, ["QA3 PG", "platypus", SRC]);
    await rowMenu(page, ["QA3 PG", "platypus"], SRC, "Check schema");
    await page.waitFor(`${lintDialog}?.querySelectorAll('.lint-findings li').length === 2`, 30_000);
    await page.eval(`${buttonIn(lintDialog, "Open 2 fixes")}.click()`);
    await page.waitFor(`${lintDialog} === null && (${queryText}).includes('CREATE INDEX')`, 15_000);
    assert.equal(await runEditorScript(page), null, await page.eval(queryText));
    assert.equal(ENGINES.pg.query(`SELECT count(*) FROM pg_indexes WHERE schemaname = '${SRC}' AND tablename = 'Order Lines'`), "3", "the two indexes were created");

    await rowMenu(page, ["QA3 PG", "platypus"], SRC, "Check schema");
    await page.waitFor(`${lintDialog} !== null && !${lintDialog}.textContent.includes('Reading the tables')`, 30_000);
    const after = await page.eval(`${lintDialog}.querySelector('.lint-findings') ? ${findings} : []`);
    assert.deepEqual(after, [], "findings remain after their own fix ran");
    assert.match(await page.eval(`${lintDialog}.textContent`), /No problems found/);
  } finally {
    ENGINES.pg.drop(SRC);
  }
}
