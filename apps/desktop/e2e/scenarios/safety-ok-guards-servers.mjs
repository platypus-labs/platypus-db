// Regression guard (passes): the editor asks before UPDATE or DELETE without WHERE and
// TRUNCATE on PostgreSQL, MySQL and SQL Server, in the forms people write (schema-qualified,
// quoted, ONLY, aliases, joins, CTEs, a WHERE only in a comment, T-SQL DELETE without FROM),
// and Cancel leaves every row; a filtered DELETE runs at once. With the Safety setting "Never
// run" the confirmation only offers Close. Each database runs when its container is up.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { connectionToml, dbExec, needs, openEditor, runOrGuard } from "../safety-helpers.mjs";
import { settingsFile } from "../settings.mjs";

const CASES = {
  postgres: {
    setup: "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h; CREATE TABLE qa_safe_g (id int primary key, v text); CREATE TABLE qa_safe_h (id int)",
    fill: "DELETE FROM qa_safe_g WHERE true; INSERT INTO qa_safe_g VALUES (1,'a'),(2,'b'),(3,'c'); DELETE FROM qa_safe_h WHERE true; INSERT INTO qa_safe_h VALUES (1)",
    unfiltered: [
      "DELETE FROM qa_safe_g",
      "delete from public.qa_safe_g",
      "DELETE FROM ONLY qa_safe_g",
      'DELETE FROM "qa_safe_g"',
      "UPDATE qa_safe_g SET v = 'x'",
      "UPDATE qa_safe_g AS g SET v = 'x'",
      "TRUNCATE qa_safe_g",
      "WITH x AS (SELECT 1) DELETE FROM qa_safe_g",
      "DELETE FROM qa_safe_g -- WHERE id = 1",
      "DELETE FROM qa_safe_g USING qa_safe_h",
      "UPDATE qa_safe_g SET v = 'x' FROM qa_safe_h",
    ],
    drop: "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h",
  },
  mysql: {
    setup: "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h; CREATE TABLE qa_safe_g (id int primary key, v text); CREATE TABLE qa_safe_h (id int)",
    fill: "DELETE FROM qa_safe_g WHERE 1; INSERT INTO qa_safe_g VALUES (1,'a'),(2,'b'),(3,'c'); DELETE FROM qa_safe_h WHERE 1; INSERT INTO qa_safe_h VALUES (1)",
    unfiltered: [
      "DELETE FROM qa_safe_g",
      "DELETE FROM platypus.qa_safe_g",
      "DELETE FROM `qa_safe_g`",
      "DELETE qa_safe_g FROM qa_safe_g",
      "UPDATE qa_safe_g SET v = 'x'",
      "UPDATE qa_safe_g g JOIN qa_safe_h h SET g.v = 'x'",
      "TRUNCATE qa_safe_g",
      "DELETE FROM qa_safe_g # WHERE id = 1",
    ],
    drop: "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h",
  },
  sqlserver: {
    setup: "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h; CREATE TABLE qa_safe_g (id int primary key, v nvarchar(10)); CREATE TABLE qa_safe_h (id int)",
    fill: "DELETE FROM qa_safe_g WHERE 1=1; INSERT INTO qa_safe_g VALUES (1,'a'),(2,'b'),(3,'c'); DELETE FROM qa_safe_h WHERE 1=1; INSERT INTO qa_safe_h VALUES (1)",
    unfiltered: ["DELETE FROM qa_safe_g", "DELETE qa_safe_g", "DELETE FROM dbo.qa_safe_g", "DELETE FROM [qa_safe_g]", "UPDATE qa_safe_g SET v = 'x'", "TRUNCATE TABLE qa_safe_g"],
    drop: "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h",
  },
};
const UNCHANGED = "SELECT count(*) FROM qa_safe_g WHERE v IN ('a', 'b', 'c')";
let kinds = [];

export async function skip() {
  kinds = [];
  const wanted = process.env.QA_SAFE_KINDS?.split(",") ?? Object.keys(CASES);
  for (const k of wanted) if (!(await needs(k)())) kinds.push(k);
  return kinds.length ? null : "no PostgreSQL, MySQL or SQL Server running";
}

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${kinds.map((k, i) => connectionToml(k, `00000000-0000-4000-8000-00000000056${i}`, `QA ${k}`)).join("")}`);
  for (const k of kinds) dbExec(k, CASES[k].setup);
}

export default async function (page, { dir }) {
  for (const k of kinds) {
    const c = CASES[k];
    try {
      await openEditor(page, `QA ${k}`);
      for (const sql of c.unfiltered) {
        dbExec(k, c.fill);
        const r = await runOrGuard(page, sql);
        assert.match(r, /^guard: Change every row\?/, `${k}: ${sql} ran without asking: ${r}`);
        assert.equal(dbExec(k, UNCHANGED), "3", `${k}: ${sql}`);
      }
      dbExec(k, c.fill);
      assert.equal(await runOrGuard(page, "DELETE FROM qa_safe_g WHERE id = 1"), "1 row affected");
    } finally {
      dbExec(k, c.drop);
    }
  }
  // "Never run", chosen in the Settings: only Close.
  if (kinds.includes("postgres")) {
    dbExec("postgres", CASES.postgres.setup);
    try {
      await page.key(",", "Comma", 188, CTRL);
      await page.settingsTab("Safety");
      await page.eval(`(() => {
        const el = [...document.querySelectorAll('.modal.settings select')].find(s => [...s.options].some(o => o.value === 'block'));
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, 'block');
        el.dispatchEvent(new Event('change', { bubbles: true }));
      })()`);
      await settingsFile(dir, /unfiltered = "block"/);
      await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
      await page.waitFor(`document.querySelector('.modal.settings') === null`);
      await openEditor(page, "QA postgres");
      dbExec("postgres", CASES.postgres.fill);
      const r = await runOrGuard(page, "DELETE FROM qa_safe_g").catch((e) => `error: ${e.message}`);
      assert.match(r, /do not allow running it/, r);
      assert.equal(dbExec("postgres", UNCHANGED), "3");
    } finally {
      dbExec("postgres", CASES.postgres.drop);
    }
  }
}
