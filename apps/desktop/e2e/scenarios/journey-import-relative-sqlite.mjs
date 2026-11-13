// JRN-01 (high). A DBeaver project whose SQLite connections have a relative path
// (`jdbc:sqlite:rossi-prod.db`, as DBeaver writes for files in its workspace) is imported
// with the path as it is ("rossi-prod.db"). Opening the connection then resolves it against
// the app's working directory (the install folder, System32, wherever it was started from)
// and, the folder existing, silently creates an empty database there: the production
// connection of the import shows "No objects" as if the customer's database were empty,
// and a stray rossi-prod.db is left in the working directory.
// Steps: first start; Import connections > two-projects.dbp > Import 4; project "Cliente
// Rossi"; expand rossi-prod (type Production).
// Expected: the import says the path is relative (or asks for the file), and opening a
// connection whose file does not exist fails with "file not found"; no file created.
// Actual: an empty database is created in the working directory and shown as the connection.
// Suspected: crates/platypus-store/src/import.rs (the JDBC path kept relative) and
// crates/platypus-driver-sqlite/src/lib.rs `open` (`in_a_folder` is true for a bare file
// name, so SQLITE_OPEN_CREATE is set).
import assert from "node:assert/strict";
import { existsSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { treeRow } from "../journey-helpers.mjs";

const FIXTURE = resolve(dirname(fileURLToPath(import.meta.url)), "../fixtures/two-projects.dbp");
export const ready = `document.querySelector('.sidebar-header') !== null`;
const dialog = `document.querySelector('.import-dialog')`;
// The app runs in the harness's working directory.
const stray = join(process.cwd(), "rossi-prod.db");

export function setup() {
  // Left by an earlier run of this scenario.
  rmSync(stray, { force: true });
}

export default async function (page) {
  // The app holds the file until it is ended: removed when the run is over.
  process.once("exit", () => rmSync(stray, { force: true }));
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(FIXTURE)})`);
  await page.eval(`document.querySelector('.sidebar-header button[title^="Import connections"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await page.eval(`${dialog}.querySelector('.import-choose').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.import-row.connection').length === 4`, 15_000);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Import 4').click()`);
  await page.waitFor(`${dialog} === null`);
  await page.eval(`document.querySelector('.project-square[title="Cliente Rossi"]').click()`);
  await page.waitFor(`${treeRow("rossi-prod")} !== undefined`);
  await page.eval(`${treeRow("rossi-prod")}.querySelector('.twisty').click()`);
  // Either the schema (the database was opened), an error row, or the notice of a failed
  // connect (the connection then folds again) shows.
  const refused = `[...document.querySelectorAll('.notice')].some(n => n.textContent.includes('rossi-prod.db not found'))`;
  await page.waitFor(`${treeRow("main")} !== undefined || document.querySelector('.tree-row.failed') !== null || ${refused}`, 15_000);
  assert.equal(existsSync(stray), false, `an empty database was created at ${stray}`);
  assert.equal(await page.eval(`${treeRow("main")} === undefined`), true, "an empty database is shown for the imported production connection");
}
