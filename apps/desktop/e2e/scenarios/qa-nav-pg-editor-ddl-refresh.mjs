// Bug NAV: DDL run in the SQL editor leaves the navigator stale. After `CREATE TABLE` in an
// editor tab the new table does not appear under its (expanded) schema, and after
// `ALTER TABLE … RENAME` / `DROP TABLE` the old name stays listed, until a manual Refresh.
// navigatorEvents.ts says the refresh is requested for "DDL run from an action, an object tab,
// the editor", but QueryTab never calls requestNavigatorRefresh (only the object actions, the
// table editor, import, migrate and fake data do). Opening the stale row then fails.
//
// Steps: expand the schema, New SQL script, run `CREATE TABLE qa_nav_ddl.qa_nav_created (id int)`.
// Expected: qa_nav_created listed under qa_nav_ddl › Tables. Actual: not listed until Refresh.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, menu, needs, row, runSql, saveConnections, visibleTab } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = "qa_nav_ddl";

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA}; CREATE TABLE ${SCHEMA}.qa_nav_old (id int);`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0013"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, SCHEMA, "qa_nav_old");
    await menu(page, "QA PG", "New SQL script");
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);

    await runSql(page, `CREATE TABLE ${SCHEMA}.qa_nav_created (id int)`);
    const created = await page
      .waitFor(`${row("qa_nav_created")} !== undefined`, 5000)
      .then(() => true, () => false);

    await runSql(page, `DROP TABLE ${SCHEMA}.qa_nav_old`);
    const dropped = await page
      .waitFor(`${row("qa_nav_old")} === undefined`, 5000)
      .then(() => true, () => false);

    assert.ok(created, "the table created in the editor is listed");
    assert.ok(dropped, "the table dropped in the editor is gone");
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;`);
  }
}
