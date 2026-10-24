// Projects: each lists its own connections in the navigator and keeps its own tabs, which
// stay open (with their sessions) while another project is shown. Ctrl+Shift+T reopens the
// active project's closed tabs, and the active project survives a reload. A workspace saved
// by 0.2.0 comes back as the tabs of All connections.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";

const ROSSI = "00000000-0000-4000-9000-000000000001";
const INTERNO = "00000000-0000-4000-9000-000000000002";
const CONN = { a: "00000000-0000-4000-8000-0000000000a1", b: "00000000-0000-4000-8000-0000000000b1", shared: "00000000-0000-4000-8000-0000000000c1" };

const tabCount = `document.querySelectorAll('.tab').length`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const editorText = `(${visibleTab}?.querySelector('.cm-content')?.textContent ?? '')`;

export function setup({ dir }) {
  const connections = ["A dev", "B dev", "Shared"].map((name, i) => {
    const file = join(dir, `${i}.db`);
    const db = new DatabaseSync(file);
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'x');");
    db.close();
    const id = [CONN.a, CONN.b, CONN.shared][i];
    return `[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${file}'\nsave_password = false\n`;
  });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "${ROSSI}"
name = "Cliente Rossi"
color = "#d08f20"
connections = ["${CONN.a}", "${CONN.shared}"]

[[projects]]
id = "${INTERNO}"
name = "Interno"
color = "#3d7bf7"
connections = ["${CONN.b}", "${CONN.shared}"]

${connections.join("\n")}`,
  );
}

/** The connections the navigator lists, by name. */
async function navigator(page) {
  return page.eval(
    `[...document.querySelectorAll('.tree-row')].map(r => r.querySelector('.tree-label')?.textContent).filter(l => ['A dev', 'B dev', 'Shared'].includes(l))`,
  );
}

async function switchProject(page, id) {
  await page.eval(`window.__platypus.switchProject(${JSON.stringify(id)})`);
}

async function openEditor(page, name, sql) {
  await page.eval(`document.querySelectorAll('#e2e-conn').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 'e2e-conn'`);
  const before = await page.eval(tabCount);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabCount} === ${before + 1}`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
}

export default async function (page) {
  // All connections at the start.
  await page.waitFor(`window.__platypus !== undefined && ${treeRow("Shared")} !== undefined`);
  assert.deepEqual(await navigator(page), ["A dev", "B dev", "Shared"]);

  // A project lists its own connections.
  await switchProject(page, ROSSI);
  await page.waitFor(`${treeRow("B dev")} === undefined`);
  assert.deepEqual(await navigator(page), ["A dev", "Shared"]);

  // Its tabs stay while another project is shown, with what was typed.
  await openEditor(page, "A dev", "SELECT 'rossi'");
  await switchProject(page, INTERNO);
  await page.waitFor(`${tabCount} === 0 && ${treeRow("B dev")} !== undefined`);
  await switchProject(page, ROSSI);
  await page.waitFor(`${tabCount} === 1`);
  assert.equal(await page.eval(editorText), "SELECT 'rossi'");

  // Ctrl+Shift+T reopens the tabs closed in the active project only.
  await switchProject(page, INTERNO);
  await page.waitFor(`${tabCount} === 0`);
  await openEditor(page, "B dev", "SELECT 'interno'");
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`${tabCount} === 0`);
  await switchProject(page, ROSSI);
  await page.waitFor(`${tabCount} === 1`);
  await page.key("T", "KeyT", 84, CTRL | SHIFT);
  await page.sleep(300);
  assert.equal(await page.eval(tabCount), 1);
  await switchProject(page, INTERNO);
  await page.waitFor(`${tabCount} === 0`);
  await page.key("T", "KeyT", 84, CTRL | SHIFT);
  await page.waitFor(`${tabCount} === 1`);
  assert.equal(await page.eval(editorText), "SELECT 'interno'");

  // The active project and its tabs come back after a reload.
  await switchProject(page, ROSSI);
  await page.waitFor(`${tabCount} === 1 && ${editorText} === "SELECT 'rossi'"`);
  await page.sleep(600); // let the debounced save pass
  await page.eval(`location.reload()`);
  await page.waitFor(`window.__platypus !== undefined && ${treeRow("A dev")} !== undefined`, 15_000);
  await page.waitFor(`${tabCount} === 1 && ${editorText} === "SELECT 'rossi'"`);
  assert.equal(await page.eval(`${treeRow("B dev")} === undefined`), true);

  // A workspace saved by 0.2.0: its tabs are All connections', and it is replaced.
  await page.eval(`localStorage.removeItem('platypus.workspace.v2'); localStorage.removeItem('platypus.project.v1');
    localStorage.setItem('platypus.workspace.v1', JSON.stringify({
      tabs: [
        { kind: 'query', connectionId: '${CONN.a}', sql: 'SELECT 1', filePath: null, savedSql: null },
        { kind: 'query', connectionId: '${CONN.b}', sql: 'SELECT 2', filePath: null, savedSql: null },
      ],
      active: 1,
    }))`);
  // Nothing may save the old tabs again before the reload.
  await page.eval(`window.addEventListener('beforeunload', () => localStorage.removeItem('platypus.workspace.v2'))`);
  await page.eval(`location.reload()`);
  await page.waitFor(`window.__platypus !== undefined && ${treeRow("B dev")} !== undefined`, 15_000);
  await page.waitFor(`${tabCount} === 2`);
  assert.equal(await page.eval(editorText), "SELECT 2");
  await page.waitFor(`localStorage.getItem('platypus.workspace.v1') === null && localStorage.getItem('platypus.workspace.v2') !== null`);
}
