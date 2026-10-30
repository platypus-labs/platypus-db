// Group D, P2 and P3 on a throwaway SQLite database: saved scripts in the Scripts panel
// (new, rename, open, save, menu, dragged into a folder and back to the top) and the visual
// query builder, a table dragged onto its canvas joining on a foreign key. The drags are
// real pointer input (press, moves, release), as a user's mouse makes them.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const scriptRow = (label) =>
  `[...document.querySelectorAll('.script-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const menuItems = `[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`;
const builderSql = `document.querySelector('.builder-tab:not([style*="none"]) .builder-sql')?.textContent ?? ''`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "e2e.db"));
  db.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
           CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
`,
  );
}

async function rightClick(page, expression) {
  await page.eval(`${expression}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  return page.eval(menuItems);
}

export default async function (page, { dir }) {
  await page.waitFor(`${row(NAME)} !== undefined`);

  // A new script is named in place, linked to the project's only connection.
  await page.eval(`document.querySelector('.scripts-panel button[aria-label="New script"]').click()`);
  await page.waitFor(`document.querySelector('.script-rename') !== null`);
  await page.eval(`document.querySelector('.script-rename').select()`);
  await page.type("report");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${scriptRow("report")} !== undefined`);
  assert.equal(await page.eval(`${scriptRow("report")}.querySelector('.tree-detail')?.textContent`), NAME);
  const file = join(dir, "scripts", "all", "report.sql");
  assert.ok(existsSync(file), file);

  // It opens in an editor that saves to its file.
  await page.eval(`${scriptRow("report")}.id = 'e2e-script'`);
  await page.clickSel("#e2e-script", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('report.sql')`, 10_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 42");
  await page.waitFor(`${editorText} === 'SELECT 42'`);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('report.sql —')`, 10_000);
  assert.equal(readFileSync(file, "utf8"), "SELECT 42");

  const items = await rightClick(page, scriptRow("report"));
  for (const item of ["Open", "Rename", "Delete…", "Unlink from its connection"]) {
    assert.ok(items.includes(item), `${item} in ${items}`);
  }
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  await dragScripts(page, dir);

  // The query builder joins a table to the one before it on their foreign key.
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("customers")} !== undefined`, 15_000);
  const tableItems = await rightClick(page, row("customers"));
  assert.ok(tableItems.includes("Visual query builder"), `${tableItems}`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Visual query builder').click()`);
  await page.waitFor(`${builderSql}.includes('FROM customers c')`, 10_000);
  await page.waitFor(`[...document.querySelectorAll('.builder-source')].some(r => r.textContent === 'orders')`, 10_000);
  const source = `[...document.querySelectorAll('.builder-source')].find(r => r.textContent === 'orders')`;
  const canvas = `document.querySelector('.builder-tab:not([style*="none"]) .builder-canvas')`;
  // Released outside the canvas: nothing is added.
  await page.dragEl(source, `document.querySelector('.builder-tab:not([style*="none"]) .builder-tables select')`);
  await page.sleep(300);
  assert.equal(await page.eval(`document.querySelectorAll('.builder-box[data-table="orders"]').length`), 0);
  // Onto the canvas: the canvas shows it takes the table, which lands under the pointer.
  let lit = false;
  await page.dragEl(source, canvas, {
    toAt: [0.6, 0.6],
    before: async () => (lit = await page.eval(`${canvas}.classList.contains('drop-into')`)),
  });
  assert.equal(lit, true);
  await page.waitFor(`${builderSql}.includes('JOIN orders o ON o.customer_id = c.id')`, 10_000);
  assert.equal(await page.eval(`${canvas}.classList.contains('drop-into')`), false);
  const box = await page.rectOf(`document.querySelector('.builder-box[data-table="orders"]')`);
  const area = await page.rectOf(canvas);
  assert.ok(box.x > area.x + area.w * 0.3, `dropped at the pointer: ${JSON.stringify({ box, area })}`);
  await page.eval(
    `[...document.querySelectorAll('.builder-box[data-table="customers"] label')].find(l => l.textContent === 'name').querySelector('input').click()`,
  );
  await page.waitFor(`${builderSql}.startsWith('SELECT c.name\\nFROM customers c')`);
  await page.eval(`[...document.querySelectorAll('.builder-bottom button')].find(b => b.textContent === 'Open in editor').click()`);
  await page.waitFor(`${visibleTab} !== undefined && ${editorText}.startsWith('SELECT c.name')`, 10_000);
}

/** A script dragged onto a folder moves into it, onto the header back to the top; a drag
 *  released elsewhere moves nothing. */
async function dragScripts(page, dir) {
  await page.eval(`document.querySelector('.scripts-panel button[aria-label="New folder"]').click()`);
  await page.waitFor(`document.querySelector('.script-rename') !== null`);
  await page.eval(`document.querySelector('.script-rename').select()`);
  await page.type("lib");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${scriptRow("lib")} !== undefined`);

  let lit = null;
  await page.dragEl(scriptRow("report"), scriptRow("lib"), {
    fromAt: [0.3, 0.5],
    toAt: [0.3, 0.5],
    before: async () => (lit = await page.eval(`document.querySelector('.script-row.drop-into')?.dataset.path`)),
  });
  assert.equal(lit, "lib");
  const moved = join(dir, "scripts", "all", "lib", "report.sql");
  await page.waitFor(`${scriptRow("report")}?.dataset.path === 'lib/report.sql'`, 10_000);
  assert.ok(existsSync(moved), moved);
  assert.equal(await page.eval(`document.querySelector('.drop-into') === null`), true);

  // Onto the folder it is in: nothing moves, nothing is outlined.
  await page.dragEl(scriptRow("report"), scriptRow("lib"), {
    fromAt: [0.3, 0.5],
    toAt: [0.3, 0.5],
    before: async () => (lit = await page.eval(`document.querySelector('.drop-into') !== null`)),
  });
  assert.equal(lit, false);
  await page.sleep(300);
  assert.ok(existsSync(moved), moved);

  // Onto the header: back to the top of the scripts.
  await page.dragEl(scriptRow("report"), `document.querySelector('.scripts-header .sidebar-title')`, {
    fromAt: [0.3, 0.5],
    before: async () => (lit = await page.eval(`document.querySelector('.scripts-header')?.classList.contains('drop-into')`)),
  });
  assert.equal(lit, true);
  await page.waitFor(`${scriptRow("report")}?.dataset.path === 'report.sql'`, 10_000);
  assert.ok(existsSync(join(dir, "scripts", "all", "report.sql")));
}
