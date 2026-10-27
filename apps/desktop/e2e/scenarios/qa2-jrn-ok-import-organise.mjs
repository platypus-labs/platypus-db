// QA journey (works, regression guard): a DBeaver project export imported into a first, empty
// start; its connections organised (a connection moved into a new folder), the production one
// pointed at its file, a replica made read-only from the connection dialog; a change on
// production asks first, the read-only replica refuses one; after a restart of the app the
// projects, folders, placements, types, the read-only flag and the tabs are all back.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { editConnection, fill, formField, saveForm } from "../conn-helpers.mjs";
import { restart, treeRow } from "../qa-jrn.mjs";
import { openEditor, runOrGuard } from "../qa-safe-db.mjs";

const FIXTURE = resolve(dirname(fileURLToPath(import.meta.url)), "../fixtures/two-projects.dbp");
export const ready = `document.querySelector('.sidebar-header') !== null`;
const dialog = `document.querySelector('.import-dialog')`;
const labels = `[...document.querySelectorAll('.tree-row')].map(r => [r.querySelector('.tree-label')?.textContent, parseInt(r.style.paddingLeft)])`;
const menuButton = (text) => `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}))`;

function seed(path) {
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE qa_jrn_orders (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO qa_jrn_orders VALUES (1, 'a'), (2, 'b');");
  db.close();
}

async function connectionMenu(page, name, ...path) {
  await page.eval(`document.querySelectorAll('#jrn-row').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.querySelector('.tree-label').id = 'jrn-row'`);
  await page.clickSel("#jrn-row", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  for (const item of path) {
    await page.waitFor(`${menuButton(item)} !== undefined`);
    await page.eval(`${menuButton(item)}.click()`);
  }
}

export default async function (page, ctx) {
  const prodFile = join(ctx.dir, "rossi-prod-abs.db");
  const replicaFile = join(ctx.dir, "rossi-replica-abs.db");
  seed(prodFile);
  seed(replicaFile);
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

  // The files, and the replica read-only.
  await editConnection(page, "rossi-prod");
  await page.eval(fill(formField("Database file"), prodFile));
  await saveForm(page);
  await editConnection(page, "rossi-replica");
  await page.eval(fill(formField("Database file"), replicaFile));
  await page.eval(`[...document.querySelectorAll('.connection-dialog label')].find(l => l.textContent.startsWith('Read-only')).querySelector('input').click()`);
  await saveForm(page);
  // rossi-dev into a new folder.
  await connectionMenu(page, "rossi-dev", "Move to folder", "New folder…");
  await page.waitFor(`document.querySelector('.name-dialog') !== null`);
  await page.eval(`document.querySelector('.name-dialog input').focus()`);
  await page.type("Archivio");
  await page.eval(`document.querySelector('.name-dialog button[type=submit]').click()`);
  await page.waitFor(`${treeRow("Archivio")} !== undefined`);
  const tree = await page.eval(labels);

  // Production asks before a change; the read-only replica refuses it.
  await openEditor(page, "rossi-prod");
  assert.match(await runOrGuard(page, "DELETE FROM qa_jrn_orders WHERE id = 1"), /^guard:/);
  await openEditor(page, "rossi-replica");
  assert.match(await runOrGuard(page, "UPDATE qa_jrn_orders SET v = 'x' WHERE id = 1", { readOnly: true }), /^read-only:/);
  for (const file of [prodFile, replicaFile]) {
    const db = new DatabaseSync(file);
    assert.equal(db.prepare("SELECT group_concat(id || v) AS r FROM qa_jrn_orders").get().r, "1a,2b");
    db.close();
  }
  const tabs = await page.eval(`[...document.querySelectorAll('.tab')].map(t => t.textContent)`);
  const toml = readFileSync(join(ctx.dir, "connections.toml"), "utf8");
  assert.match(toml, /name = "rossi-replica"[^[]*read_only = true/);
  assert.match(toml, /name = "rossi-prod"[^[]*connection_type = "prod"/);

  // Restart: the window closed as a user closes it.
  await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`document.querySelector('.project-square.active')?.title === 'Cliente Rossi'`, 15_000);
  await page.waitFor(`${treeRow("Archivio")} !== undefined`, 15_000);
  assert.deepEqual(await page.eval(labels), tree);
  await page.waitFor(`document.querySelectorAll('.tab').length === ${tabs.length}`, 15_000);
  assert.deepEqual(await page.eval(`[...document.querySelectorAll('.tab')].map(t => t.textContent)`), tabs);
  assert.equal(await page.eval(`${treeRow("rossi-prod")}.textContent.includes('PROD')`), true);
}
