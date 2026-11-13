// SCH-14 (low). With the interface in Italian, the script Confronta con… opens is headed in
// Italian ("-- Rende … uguale a …") but its warnings stay in English: "-- Drops the table
// gone: its rows are lost.", "-- SQLite cannot make these changes in place: the table is
// created again and its rows copied." (the table editor shows the same warnings in English).
// Steps: Settings > Lingua Italiano; qa3s_src has t(id, name), qa3s_dst t(id) and gone;
// navigator > main > Confronta con… > Confronta.
// Expected: the warnings in Italian. Actual: in English.
// Suspected: crates/platypus-schema/src/compare.rs:73 and ddl.rs (warnings built as English
// text, with no template for the UI to translate, unlike the linter's findings) and
// apps/desktop/src/schema/CompareDialog.tsx:53 (written as they come).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { italian } from "../journey-helpers.mjs";
import { DST, ENGINES, SRC, buttonIn, compareDialog, openPath, queryText, rowMenu, saveEngines } from "../schema-compare-helpers.mjs";

const IT = italian();

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x');`);
  ENGINES.sqlite.seed(dir, DST, `CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); CREATE TABLE gone (id INTEGER PRIMARY KEY);`);
  saveEngines(dir, ["sqlite"]);
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", IT["Compare with…"].replace("…", ""));
  await page.waitFor(`${compareDialog}?.querySelectorAll('select')[1]?.value === 'main'`, 15_000);
  // The dialog itself is Italian.
  const dialog = await page.eval(`${compareDialog}.textContent`);
  assert.ok(dialog.includes(IT["Compare"]) && dialog.includes(IT["Cancel"]), dialog);
  await page.eval(`${buttonIn(compareDialog, IT["Compare"])}.click()`);
  await page.waitFor(`${compareDialog} === null && (${queryText}).includes('DROP TABLE')`, 15_000);
  const comments = (await page.eval(queryText)).split("\n").filter((l) => l.startsWith("--"));
  assert.match(comments[0], /uguale a/);
  for (const c of comments) assert.doesNotMatch(c, /\b(Drops the table|its rows are lost|cannot make these changes|created again)\b/, `English in the Italian script: ${c}`);
}
