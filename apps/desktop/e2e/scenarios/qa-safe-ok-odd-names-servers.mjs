// Regression guard (passes): the data editor on PostgreSQL, MySQL and SQL Server tables whose
// names hold each database's quote character and a single quote, saving a value with
// backslashes, quotes, backticks and brackets: the right row of the right table changes, the
// value is stored unchanged (MySQL's backslashes escaped), and a deleted row goes. Each
// database runs when its container is up; skipped when none is.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { connectionToml, dbExec, needs, openEditor, status, visibleTab } from "../qa-safe-db.mjs";

const CASES = {
  postgres: { table: `qa_safe_q"t'x`, col: `c"ol'x`, q: (s) => `"${s.replaceAll('"', '""')}"`, type: "text", sep: "|" },
  mysql: { table: "qa_safe_b`t'x", col: "c`ol'x", q: (s) => `\`${s.replaceAll("`", "``")}\``, type: "text", sep: "\t" },
  sqlserver: { table: "qa_safe_[br]'x", col: "c]ol'x", q: (s) => `[${s.replaceAll("]", "]]")}]`, type: "nvarchar(200)", sep: " " },
};
const VALUE = "C:\\temp\\new it's \"q\" `b` [s]";
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
let kinds = [];

export async function skip() {
  kinds = [];
  for (const k of Object.keys(CASES)) if (!(await needs(k)())) kinds.push(k);
  return kinds.length ? null : "no PostgreSQL, MySQL or SQL Server running";
}

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${kinds.map((k, i) => connectionToml(k, `00000000-0000-4000-8000-00000000053${i}`, `QA ${k}`)).join("")}`);
  for (const k of kinds) {
    const c = CASES[k];
    dbExec(k, `DROP TABLE IF EXISTS ${c.q(c.table)}`);
    dbExec(k, `CREATE TABLE ${c.q(c.table)} (id int primary key, ${c.q(c.col)} ${c.type})`);
    dbExec(k, `INSERT INTO ${c.q(c.table)} VALUES (1, 'a'), (2, 'b'), (3, 'c')`);
  }
}

/** The stored value as the database's client prints it: MySQL's batch mode escapes backslashes. */
const printed = (k, v) => (k === "mysql" ? v.replaceAll("\\", "\\\\") : v);

export default async function (page) {
  for (const k of kinds) {
    const c = CASES[k];
    try {
      await openEditor(page, `QA ${k}`);
      const sql = `SELECT * FROM ${c.q(c.table)} ORDER BY id`;
      await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
      await page.key("a", "KeyA", 65, CTRL);
      await page.type(sql);
      await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
      await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
      await page.key("Enter", "Enter", 13, CTRL);
      await page.waitFor(`${status} === '3 rows'`, 20_000);
      await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
      await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
      await page.key("ArrowDown", "ArrowDown", 40);
      await page.key("ArrowRight", "ArrowRight", 39);
      await page.key("F2", "F2", 113);
      await page.waitFor(`document.querySelector('.cell-editor') !== null`);
      await page.key("a", "KeyA", 65, CTRL);
      await page.type(VALUE);
      await page.key("Enter", "Enter", 13);
      await page.waitFor(`document.querySelector('.cell-editor') === null`);
      await page.key("Delete", "Delete", 46);
      await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
      await page.key("s", "KeyS", 83, CTRL);
      await page.waitFor(`${editBar}.includes('to edit a cell') && !/changed/.test(${editBar})`, 15_000);
      await page.waitFor(`${visibleTab}.querySelector('.statusbar')?.textContent.includes('Changes saved')`, 15_000);
      assert.equal(dbExec(k, `SELECT id, ${c.q(c.col)} FROM ${c.q(c.table)} ORDER BY id`), `1${c.sep}${printed(k, VALUE)}\n3${c.sep}c`, k);
    } finally {
      dbExec(k, `DROP TABLE IF EXISTS ${c.q(c.table)}`);
    }
  }
}
