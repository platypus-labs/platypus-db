// Bug NAV: on MySQL, tables named with MySQL reserved words that are not in the shared list
// (`rank`, `range`, `interval`, and others such as `groups`, `lead`, `read`, `change`, `match`,
// `release`, `div`, `mod`, `system`, `condition`) are written bare. In the connection's
// database, Open data runs `SELECT * FROM rank` and fails with error 1064, and Generate SELECT,
// Drop… and Truncate… produce the same broken SQL. (Qualified with another database,
// `db.rank` happens to work, so the bug only shows in the saved database.)
//
// Cause: `quoteIdent` (apps/desktop/src/sqlNames.ts) and `Dialect::quote_ident`
// (crates/platypus-sql/src/dialect.rs) check one shared RESERVED list plus T-SQL and Oracle
// lists, but have no MySQL list (the same lack likely affects column names in generated
// UPDATE/INSERT and in the grid's save).
//
// Steps: connection to database qa_nav_rw; right-click `rank` › Open data.
// Expected: the row of `rank` (`SELECT * FROM \`rank\``). Actual: 1064 syntax error near 'rank'.
import assert from "node:assert/strict";
import { DBS, connectTo, editorText, menu, needs, row, saveConnections, visibleTab, waitRun } from "../qa-nav-helpers.mjs";

export const skip = needs("mysql");

const DB = "qa_nav_rw";

export function setup({ dir }) {
  DBS.mysql.exec(`DROP DATABASE IF EXISTS ${DB}; CREATE DATABASE ${DB}; GRANT ALL ON ${DB}.* TO 'platypus'@'%';
    CREATE TABLE ${DB}.\`rank\` (id int PRIMARY KEY, \`range\` int);
    CREATE TABLE ${DB}.\`interval\` (id int);
    INSERT INTO ${DB}.\`rank\` VALUES (1, 10);`);
  saveConnections(dir, DBS.mysql.connection("QA MY", "00000000-0000-4000-8000-0000000a0021").replace('database = "platypus"', `database = "${DB}"`));
}

export default async function (page) {
  try {
    await connectTo(page, "QA MY", "platypus");
    await page.waitFor(`${row("rank")} !== undefined`, 20_000);

    await menu(page, "rank", "Open data");
    await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`, 10_000);
    const status = await waitRun(page);
    const sql = await page.eval(editorText);
    assert.equal(status, "1 row", `Open data ran: ${sql}`);

    await menu(page, "interval", "Drop…");
    await page.waitFor(`/DROP TABLE/.test(document.querySelector('.modal.sql-action .sql-preview')?.textContent ?? '')`, 10_000);
    const drop = await page.eval(`document.querySelector('.modal.sql-action .sql-preview').textContent`);
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Cancel').click()`);
    assert.match(drop, /DROP TABLE `interval`;/);
  } finally {
    DBS.mysql.exec(`DROP DATABASE IF EXISTS ${DB};`);
  }
}
