// Bug NAV: on PostgreSQL, Open data of a table in the current schema named like an SQL value
// function (`current_role`, `current_schema`, `current_catalog`, `session_user`,
// `system_user`, `localtime`, `localtimestamp`) shows the function's value instead of the
// table: the viewer runs `SELECT * FROM current_role`, which PostgreSQL reads as the function,
// and the grid shows one row `platypus` — silently wrong data, no error. Generated
// UPDATE/DELETE and Drop… are unquoted too (`DROP TABLE current_role` fails).
//
// Cause: the reserved lists in apps/desktop/src/sqlNames.ts (`RESERVED`) and
// crates/platypus-sql/src/dialect.rs miss PostgreSQL's reserved CURRENT_ROLE,
// CURRENT_CATALOG, SESSION_USER, SYSTEM_USER, LOCALTIME, LOCALTIMESTAMP (and current_schema,
// which is not reserved but parses as the function without parentheses in FROM).
//
// Steps: connection to database qa_nav_pgdb (schema public current); right-click the table
// current_role › Open data.
// Expected: the table's row `table data` under column `secret`. Actual: column `current_role`
// with the user name.
import assert from "node:assert/strict";
import { DBS, connectTo, editorText, gridText, menu, needs, row, saveConnections, visibleTab, waitRun } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

const DB = "qa_nav_pgdb";

export function setup({ dir }) {
  DBS.postgres.exec(`DROP DATABASE IF EXISTS ${DB};`);
  DBS.postgres.exec(`CREATE DATABASE ${DB};`);
  DBS.postgres.exec(`CREATE TABLE "current_role" (secret text); INSERT INTO "current_role" VALUES ('table data');`, DB);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0014").replace('database = "platypus"', `database = "${DB}"`));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await page.waitFor(`${row("current_role")} !== undefined`, 20_000);
    await menu(page, "current_role", "Open data");
    await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`, 10_000);
    await waitRun(page);
    const data = await gridText(page);
    assert.equal(data, "secret\ntable data", `grid of ${JSON.stringify(await page.eval(editorText))}`);
  } finally {
    // The app's sessions may still be open on the database: end them first.
    DBS.postgres.exec(`SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${DB}' AND pid <> pg_backend_pid();`);
    DBS.postgres.exec(`DROP DATABASE IF EXISTS ${DB};`);
  }
}
