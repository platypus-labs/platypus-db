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
  // For the connection made in the scenario.
  new DatabaseSync(join(dir, "3.db")).close();
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
  await members(page, dir, shots);
  await follows(page);
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
  const r = await page.rect(`.project-square[title="Personale"]`);
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.includes('Delete project'))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.includes('Delete project')).click()`);
  await answer(page, true);
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

const menuButton = (label) => `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.includes(${JSON.stringify(label)}))`;

async function rowMenu(page, name) {
  await page.eval(`document.querySelectorAll('#e2e-row').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 'e2e-row'`);
  const r = await page.rect("#e2e-row");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

/** Connections joining and leaving projects, and what happens to tabs and projects when
 *  connections or projects go. Starts in Interno. */
async function members(page, dir, shots) {
  // The connection dialog sets the projects of a connection.
  await page.eval(`[...${treeRow("B dev")}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  await page.waitFor(`document.querySelector('.project-chips') !== null`);
  const chip = (name) => `[...document.querySelectorAll('.project-chip')].find(c => c.textContent === ${JSON.stringify(name)})`;
  assert.equal(await page.eval(`${chip("Cliente Rossi")}.getAttribute('aria-pressed')`), "false");
  assert.equal(await page.eval(`${chip("Interno")}.getAttribute('aria-pressed')`), "true");
  await page.eval(`${chip("Cliente Rossi")}.click()`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.project-chips') === null`);
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`${activeSquare} === '${ROSSI}' && ${treeRow("B dev")} !== undefined`);

  // A connection made in a project is in that project.
  await page.key("3", "Digit3", 51, CTRL);
  await page.waitFor(`${activeSquare} === '${INTERNO}'`);
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  // A new connection starts from the choice of the database.
  await page.waitFor(`document.querySelector('.driver-grid') !== null`);
  assert.deepEqual(await page.eval(`[...document.querySelectorAll('.driver-tile')].map(b => b.textContent)`), [
    "PostgreSQL",
    "MySQL",
    "MariaDB",
    "SQL Server",
    "ClickHouse",
    "SQLite",
    "DuckDB",
    "Turso / libSQL",
    "MongoDB",
    "Snowflake",
    "BigQuery",
    "Databricks",
    "Trino",
    "Other ADBC driver",
    "Oracle",
    "IBM Db2",
    "Other ODBC driver",
    "CockroachDB",
    "YugabyteDB",
    "TimescaleDB",
    "Supabase",
    "Neon",
    "AlloyDB",
  ]);
  // The search field has the focus, filters the tiles, Esc clears it and Enter picks the last one.
  const tiles = `[...document.querySelectorAll('.driver-tile')].map(b => b.textContent)`;
  assert.equal(await page.eval(`document.activeElement === document.querySelector('.driver-search')`), true);
  await page.type("SQL");
  await page.waitFor(`${tiles}.join() === 'SQL Server,SQLite'`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.driver-search').value === '' && ${tiles}.length === 23`);
  await page.type("sqlite");
  await page.waitFor(`${tiles}.join() === 'SQLite'`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.project-chips') !== null`);
  assert.equal(await page.eval(`${chip("Interno")}.getAttribute('aria-pressed')`), "true");
  await page.eval(`(() => {
    const set = (el, value) => {
      const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
    };
    const fields = [...document.querySelectorAll('.modal .field')];
    set(fields.find(f => f.textContent.startsWith('Name')).querySelector('input'), 'C new');
    set(fields.find(f => f.textContent.startsWith('Driver')).querySelector('select'), 'sqlite');
  })()`);
  await page.waitFor(`[...document.querySelectorAll('.modal .field')].some(f => f.textContent.startsWith('Database file'))`);
  await page.eval(`(() => {
    const input = [...document.querySelectorAll('.modal .field')].find(f => f.textContent.startsWith('Database file')).querySelector('input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(join(dir, "3.db"))});
    input.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`${treeRow("C new")} !== undefined`);
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`${activeSquare} === '${ROSSI}' && ${treeRow("C new")} === undefined`);

  // Out of the project: gone from its navigator, its open tab stays, still in All connections.
  assert.equal(await page.eval(tabCount), 1);
  await rowMenu(page, "A dev");
  await page.eval(`${menuButton("Remove from this project")}.click()`);
  await page.waitFor(`${treeRow("A dev")} === undefined`);
  assert.equal(await page.eval(tabCount), 1);
  // And back in, from the navigator's menu in All connections.
  await page.key("1", "Digit1", 49, CTRL);
  await page.waitFor(`${activeSquare} === 'all' && ${treeRow("A dev")} !== undefined`);
  await rowMenu(page, "A dev");
  await page.eval(`${menuButton("Add to project")}.click()`);
  await page.waitFor(`${menuButton("Cliente Rossi")} !== undefined`);
  await page.eval(`${menuButton("Cliente Rossi")}.click()`);
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`${activeSquare} === '${ROSSI}' && ${treeRow("A dev")} !== undefined`);

  // An empty project offers to fill it.
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await page.type("Vuoto");
  await page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.project-empty')?.textContent.includes('No connections in this project')`);
  if (shots) await page.screenshot(join(shots, "project-empty.png"));
  assert.deepEqual(await page.eval(`[...document.querySelectorAll('.project-empty button')].map(b => b.textContent)`), [
    "New connection",
    "Add existing…",
    "Import…",
  ]);
  await page.eval(`[...document.querySelectorAll('.project-empty button')].find(b => b.textContent === 'Add existing…').click()`);
  await page.waitFor(`document.querySelector('.add-connections') !== null`);
  await page.eval(`[...document.querySelectorAll('.add-connections label')].find(l => l.textContent === 'Shared').querySelector('input').click()`);
  await page.eval(`[...document.querySelectorAll('.add-connections button')].find(b => b.textContent.startsWith('Add')).click()`);
  await page.waitFor(`${treeRow("Shared")} !== undefined && document.querySelector('.project-empty') === null`);

  // A deleted connection leaves every project.
  await page.key("1", "Digit1", 49, CTRL);
  await page.waitFor(`${activeSquare} === 'all'`);
  await rowMenu(page, "Shared");
  await page.eval(`${menuButton("Delete")}.click()`);
  await answer(page, true);
  await page.waitFor(`${treeRow("Shared")} === undefined`);
  for (const key of ["2", "3"]) {
    await page.key(key, `Digit${key}`, 48 + Number(key), CTRL);
    await page.waitFor(`${activeSquare} !== 'all'`);
    assert.equal(await page.eval(`${treeRow("Shared")} === undefined`), true);
    await page.key("1", "Digit1", 49, CTRL);
    await page.waitFor(`${activeSquare} === 'all'`);
  }
  assert.doesNotMatch(readFileSync(join(dir, "connections.toml"), "utf8"), new RegExp(CONN.shared));

  // A project deleted while its tabs hold changes asks first; saying no keeps it.
  await page.key("3", "Digit3", 51, CTRL);
  await page.waitFor(`${activeSquare} === '${INTERNO}'`);
  await openEditor(page, "B dev", "SELECT 1");
  await page.eval(`${visibleTab}.querySelector('button[aria-label="Auto-commit"]').click()`);
  await page.waitFor(`${visibleTab}.querySelector('button[aria-label="Manual commit"]') !== null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("UPDATE t SET v = 'z' WHERE id = 1");
  await page.waitFor(`${editorText} === "UPDATE t SET v = 'z' WHERE id = 1"`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${visibleTab}.querySelector('.tx-open') !== null`, 15_000);
  const deleteInterno = async (answers) => {
    const r = await page.rect(`.project-square[data-project="${INTERNO}"]`);
    await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
    await page.waitFor(`${menuButton("Delete project")} !== undefined`);
    await page.eval(`${menuButton("Delete project")}.click()`);
    for (const yes of answers) await answer(page, yes);
  };
  await deleteInterno([true, false]);
  await page.sleep(300);
  assert.equal(await page.eval(`${square(INTERNO)} !== null && ${activeSquare} === '${INTERNO}'`), true);
  assert.equal(await page.eval(tabCount), 1);
  // Closing the tab with its transaction open asks in the app's own dialog; Cancel keeps it.
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`document.querySelector('.modal.confirm p')?.textContent.includes('open transaction')`);
  // The shortcuts wait while it asks: a second Ctrl+W opens no second question.
  await page.key("w", "KeyW", 87, CTRL);
  await page.sleep(200);
  assert.equal(await page.eval(`document.querySelectorAll('.modal.confirm').length`), 1);
  await answer(page, false);
  assert.equal(await page.eval(tabCount), 1);
  await deleteInterno([true, true]);
  await page.waitFor(`${square(INTERNO)} === null && ${activeSquare} === 'all'`);
  // All connections still has its two tabs; Interno's closed with it.
  await page.waitFor(`${tabCount} === 2`);
}

/** Answers the app's yes/no dialog (ui/ask.tsx): its confirming button, or Cancel. */
async function answer(page, yes) {
  // Another question may follow at once: wait for this one to go, not for none.
  await page.waitFor(`document.querySelector('.modal.confirm:not([data-answered])') !== null`);
  await page.eval(`(() => {
    const dialog = document.querySelector('.modal.confirm:not([data-answered])');
    dialog.dataset.answered = '';
    [...dialog.querySelectorAll('button')].find(b => (b.textContent === 'Cancel') === ${!yes}).click();
  })()`);
  await page.waitFor(`document.querySelector('.modal.confirm[data-answered]') === null`);
}

/** Runs `sql` in a new editor on `name` and waits for its row. */
async function runIn(page, name, sql) {
  await openEditor(page, name, sql);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 15_000);
}

/** The window title, the navigator's header and the palette follow the project. Starts in
 *  All connections, with Cliente Rossi holding A dev and B dev, and C new in no project. */
async function follows(page) {
  assert.equal(await page.eval(`document.title`), "PlatypusDB");
  await runIn(page, "C new", "SELECT 'c'");
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`document.title === 'Cliente Rossi — PlatypusDB'`);
  assert.equal(await page.eval(`document.querySelector('.navigator-project')?.textContent`), "Cliente Rossi");
  await runIn(page, "A dev", "SELECT 'a'");

  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`[...document.querySelectorAll('.palette li')].some(l => l.textContent.includes("SELECT 'a'"))`);
  const items = await page.eval(`[...document.querySelectorAll('.palette li')].map(l => l.textContent)`);
  assert.ok(items.some((t) => t.includes("Open SQL editor: A dev")), items.join(" | "));
  assert.ok(!items.some((t) => t.includes("C new") || t.includes("SELECT 'c'")), items.join(" | "));
  await page.key("Escape", "Escape", 27);
  await page.key("1", "Digit1", 49, CTRL);
  await page.waitFor(`document.title === 'PlatypusDB' && document.querySelector('.navigator-project') === null`);
}
