// Projects: each lists its own connections in the navigator and keeps its own tabs, which
// stay open (with their sessions) while another project is shown. Ctrl+Shift+T reopens the
// active project's closed tabs, and the active project survives a reload. A workspace saved
// by 0.2.0 comes back as the tabs of All connections. The rail switches projects by click
// and shortcut and marks open transactions; projects are created, refused when the name is
// taken and deleted, and the palette switches to them.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL, SHIFT } from "../cdp.mjs";

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

export default async function (page, { dir, shots }) {
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

  await rail(page);
  if (shots) await page.screenshot(join(shots, "projects-rail.png"));
  await dialogs(page, dir);
}

const square = (id) => `document.querySelector('.project-square[data-project="${id}"]')`;
const activeSquare = `document.querySelector('.project-square.active')?.dataset.project`;

/** The rail: a square per project, clicks and shortcuts switch, and an open transaction
 *  marks its project's square while another project is shown. */
async function rail(page) {
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.project-square')].map(s => [s.dataset.project, s.textContent, s.title])`),
    [
      ["all", "", "All connections"],
      [ROSSI, "CR", "Cliente Rossi"],
      [INTERNO, "IN", "Interno"],
    ],
  );
  assert.equal(await page.eval(`getComputedStyle(${square(ROSSI)}).backgroundColor`), "rgb(208, 143, 32)");
  assert.equal(await page.eval(activeSquare), "all");

  await page.eval(`${square(INTERNO)}.click()`);
  await page.waitFor(`${activeSquare} === '${INTERNO}' && ${treeRow("A dev")} === undefined`);
  await page.key("1", "Digit1", 49, CTRL);
  await page.waitFor(`${activeSquare} === 'all'`);
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`${activeSquare} === '${ROSSI}'`);
  await page.key("PageDown", "PageDown", 34, CTRL | ALT);
  await page.waitFor(`${activeSquare} === '${INTERNO}'`);

  // A transaction stays open across a switch, and the square says so.
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`${activeSquare} === '${ROSSI}' && ${tabCount} === 0`);
  await openEditor(page, "A dev", "SELECT 1");
  await page.eval(`${visibleTab}.querySelector('button[aria-label="Auto-commit"]').click()`);
  await page.waitFor(`${visibleTab}.querySelector('button[aria-label="Manual commit"]') !== null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("UPDATE t SET v = 'y' WHERE id = 1");
  await page.waitFor(`${editorText} === "UPDATE t SET v = 'y' WHERE id = 1"`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${visibleTab}.querySelector('.tx-open') !== null`, 15_000);
  await page.key("3", "Digit3", 51, CTRL);
  await page.waitFor(`${activeSquare} === '${INTERNO}' && ${square(ROSSI)}.classList.contains('tx')`);
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`${visibleTab}?.querySelector('.tx-open') != null`);
  await page.eval(`${visibleTab}.querySelector('button[aria-label="Rollback"]').click()`);
  await page.waitFor(`${visibleTab}.querySelector('.tx-open') === null && !${square(ROSSI)}.classList.contains('tx')`);

  // The shortcuts are listed with the others, to change them.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`[...document.querySelectorAll('.shortcuts tr')].some(r => r.firstChild?.textContent === 'Next project')`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
}

/** New, invalid and deleted projects, and the palette's projects. */
async function dialogs(page, dir) {
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await page.type("Personale");
  await page.waitFor(`document.querySelector('.project-initials').value === 'PE'`);
  await page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.project-square[title="Personale"]')?.textContent === 'PE'`);
  // The new project is the active one, empty.
  assert.equal(await page.eval(`document.querySelector('.project-square.active').title`), "Personale");
  assert.match(readFileSync(join(dir, "connections.toml"), "utf8"), /name = "Personale"/);

  // Names are unique, whatever the case.
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await page.type("interno");
  await page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.project-dialog .notice.error') !== null`);
  assert.equal(await page.eval(`document.querySelectorAll('.project-square').length`), 4);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.project-dialog') === null`);

  // Deleting a project keeps its connections.
  await page.eval(`window.confirm = () => true`);
  const r = await page.rect(`.project-square[title="Personale"]`);
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.includes('Delete project'))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.includes('Delete project')).click()`);
  await page.waitFor(`document.querySelector('.project-square[title="Personale"]') === null && ${activeSquare} === 'all'`);
  assert.deepEqual(await navigator(page), ["A dev", "B dev", "Shared"]);
  assert.doesNotMatch(readFileSync(join(dir, "connections.toml"), "utf8"), /Personale/);

  // The palette switches projects.
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type("Interno");
  await page.waitFor(`document.querySelector('.palette li.selected')?.textContent.includes('Interno')`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${activeSquare} === '${INTERNO}'`);
}
