// CONN-20 (low). Importing a DBeaver project export again, after ticking its connections
// (they are unticked as "already saved"), puts them in new projects "Cliente Rossi (2)" and
// "Interno (2)": the target offered first is "New project", although the projects of the
// same name exist and are in the list ("Into "Cliente Rossi"").
// Steps: import e2e/fixtures/two-projects.dbp; import it again; tick every connection;
// Import.
// Expected: the existing projects of the same name offered first; no "(2)" projects.
// Actual: two new projects with " (2)".
// Suspected: apps/desktop/src/components/ImportDialog.tsx `defaultInto`.
import assert from "node:assert/strict";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { seedSqlite } from "../conn-helpers.mjs";

const FIXTURE = resolve(dirname(fileURLToPath(import.meta.url)), "../fixtures/two-projects.dbp");
const dlg = `document.querySelector('.import-dialog')`;
const importButton = `[...${dlg}.querySelectorAll('button')].find(b => /^Import \\d/.test(b.textContent.trim()))`;

export function setup({ dir }) {
  seedSqlite(dir);
}

async function openImport(page) {
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(FIXTURE)})`);
  await page.eval(`document.querySelector('.sidebar-header button[title^="Import connections"]').click()`);
  await page.waitFor(`${dlg} !== null`);
  await page.eval(`${dlg}.querySelector('.import-choose').click()`);
  await page.waitFor(`${dlg}.querySelectorAll('.import-row.connection').length === 4`, 15_000);
}

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined`);
  await openImport(page);
  await page.waitFor(`${importButton} !== undefined`);
  await page.eval(`${importButton}.click()`);
  await page.waitFor(`${dlg} === null`);
  await page.waitFor(`document.querySelectorAll('.project-square').length === 3`);

  await openImport(page);
  await page.eval(`[...${dlg}.querySelectorAll('.import-row.connection input')].forEach(i => { if (!i.checked) i.click(); })`);
  await page.waitFor(`${importButton}?.textContent.trim() === 'Import 4'`);
  await page.eval(`${importButton}.click()`);
  await page.waitFor(`${dlg} === null`);
  const squares = await page.eval(`[...document.querySelectorAll('.project-square')].map(s => s.title)`);
  assert.deepEqual(squares.filter((s) => s.endsWith("(2)")), [], JSON.stringify(squares));
}
