// Import of a DBeaver project export (.dbp) with two projects and nested folders: the
// preview shows projects, folders and connections, an unticked connection stays out, and
// the two new projects end up in a rail folder named after the file, with their folders in
// the navigator.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const FIXTURE = resolve(dirname(fileURLToPath(import.meta.url)), "../fixtures/two-projects.dbp");
const dialog = `document.querySelector('.import-dialog')`;
const rowOf = (name) =>
  `[...${dialog}.querySelectorAll('.import-row')].find(r => r.textContent.includes(${JSON.stringify(name)}))`;
const treeRows = `[...document.querySelectorAll('.tree-row')].map(r => [r.querySelector('.tree-label')?.textContent, parseInt(r.style.paddingLeft)])`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 5

[[connections]]
id = "00000000-0000-4000-8000-000000000031"
name = "Local"
driver = "sqlite"
database = '${join(dir, "local.db")}'
save_password = false
`,
  );
}

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(FIXTURE)})`);
  await page.eval(`document.querySelector('.sidebar-header button[title^="Import connections"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await page.eval(`${dialog}.querySelector('.import-choose').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.import-row.connection').length === 4`, 15_000);

  // Two projects, their folders and connections.
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.import-row.project').length`), 2);
  assert.deepEqual(await page.eval(`[...${dialog}.querySelectorAll('.import-row.folder')].map(r => r.textContent)`), [
    "Prod",
    "Replica",
  ]);
  assert.equal(await page.eval(`${dialog}.querySelector('.import-rail-name').value`), "two-projects");

  // One connection left out.
  await page.eval(`${rowOf("rossi-dev")}.querySelector('input').click()`);
  await page.waitFor(`[...${dialog}.querySelectorAll('button')].some(b => b.textContent.trim() === 'Import 3')`);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Import 3').click()`);
  await page.waitFor(`${dialog} === null`);

  // Both projects in a rail folder, open.
  await page.waitFor(`document.querySelectorAll('.project-folder-open .project-square').length === 2`);
  const titles = await page.eval(`[...document.querySelectorAll('.project-folder-open .project-square')].map(s => s.title)`);
  assert.deepEqual(titles, ["Cliente Rossi", "Interno"]);

  // The project's folders in the navigator, nested, folders first.
  await page.eval(`document.querySelector('.project-square[title="Cliente Rossi"]').click()`);
  await page.waitFor(`${treeRows}.some(([l]) => l === 'rossi-replica')`);
  const rows = await page.eval(treeRows);
  assert.deepEqual(
    rows.map(([label]) => label),
    ["Prod", "Replica", "rossi-replica", "rossi-prod"],
  );
  const [prod, replica, inReplica, inProd] = rows.map(([, pad]) => pad);
  assert.ok(prod < replica && replica < inReplica && inProd === replica, JSON.stringify(rows));

  // Collapsing the rail folder keeps the active project marked on it.
  await page.eval(`document.querySelector('.project-folder-head').click()`);
  await page.waitFor(`document.querySelector('.project-folder.active') !== null`);
}
