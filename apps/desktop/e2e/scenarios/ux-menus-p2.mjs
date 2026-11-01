// Group T, P2 (menus, start page, rail, tabs):
// - the start page has "Open shared project…", and Export has another icon than Import;
// - the table menu keeps analysis actions in an "Analyze" submenu;
// - "Add bookmark" confirms with a toast;
// - the rail's "+" has a right-click menu (New project, Open shared project);
// - the project menu separates look (Rename, Color) from sharing, and ends with Delete;
// - the tab menu opens under the tab; the New project dialog has the X close button.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "UX p2 db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menu = `document.querySelector('.context-menu')`;
/** The menu's own lines in order: the label of a button, "---" for a separator. */
const lines = `[...${menu}.children].map(c => c.tagName === 'HR' ? '---' : (c.matches('button') ? c : c.querySelector(':scope > button'))?.querySelector('span')?.textContent)`;
const submenuLabels = `[...document.querySelectorAll('.context-menu.submenu button span:first-child')].map(s => s.textContent)`;
const card = (title) => `[...document.querySelectorAll('.welcome-card')].find(c => c.querySelector('strong')?.textContent === ${JSON.stringify(title)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ux.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000072"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "ux.db")}'
save_password = false
`,
  );
}

async function rightClick(page, selector) {
  const r = await page.rect(selector);
  await page.click(r.x + Math.min(20, r.w / 2), r.y + r.h / 2, { button: "right" });
  await page.waitFor(`${menu} !== null`);
}

async function openTableMenu(page) {
  await page.eval(`${row("t")}.querySelector('.tree-label').id = 'e2e-row'`);
  await rightClick(page, "#e2e-row");
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
}

async function closeMenu(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

export default async function (page) {
  // Start page: the shared-project card, and Import and Export do not share an icon.
  await page.waitFor(`${card("Open shared project…")} != null`);
  assert.notEqual(
    await page.eval(`${card("Import connections")}.querySelector('svg').outerHTML`),
    await page.eval(`${card("Export connections")}.querySelector('svg').outerHTML`),
    "Export has its own icon",
  );

  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("t")} !== undefined`, 15_000);

  // Table menu: analysis in a submenu, not at the first level.
  await openTableMenu(page);
  const top = await page.eval(lines);
  assert.ok(top.includes("Analyze"), `Analyze in ${top}`);
  for (const gone of ["Visual query builder", "Query across databases…", "Compare data with…"]) {
    assert.ok(!top.includes(gone), `${gone} is in the Analyze submenu, not in ${top}`);
  }
  assert.ok(top.length <= 16, `the table menu is short: ${top.length} lines`);
  await page.eval(`[...${menu}.querySelectorAll('button')].find(b => b.getAttribute('aria-haspopup') && b.textContent.startsWith('Analyze')).click()`);
  await page.waitFor(`${submenuLabels}.includes('Visual query builder')`);
  const analyze = await page.eval(submenuLabels);
  for (const label of ["Visual query builder", "Query across databases…", "Compare data with…"]) {
    assert.ok(analyze.includes(label), `${label} in ${analyze}`);
  }
  // The submenu is a fixed panel, so a scrolling parent menu cannot clip it.
  assert.equal(await page.eval(`getComputedStyle(document.querySelector('.context-menu.submenu')).position`), "fixed");
  assert.ok(await page.eval(`document.querySelector('.context-menu.submenu').getBoundingClientRect().bottom <= window.innerHeight`));
  await closeMenu(page);

  // Add bookmark confirms with a toast; the item then offers to remove it.
  await openTableMenu(page);
  await page.eval(`[...${menu}.querySelectorAll('button')].find(b => b.textContent.startsWith('Add bookmark')).click()`);
  await page.waitFor(`[...document.querySelectorAll('.toast')].some(t => t.textContent.includes('Added to bookmarks'))`);
  await openTableMenu(page);
  assert.ok((await page.eval(lines)).includes("Remove bookmark"));
  await closeMenu(page);

  // Tab menu: it opens under the tab, which stays visible.
  await page.eval(`${row("t")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.tab.active') !== null`, 15_000);
  await page.eval(`document.querySelector('.tab.active').id = 'e2e-tab'`);
  await rightClick(page, "#e2e-tab");
  assert.ok(
    await page.eval(`${menu}.getBoundingClientRect().top >= document.getElementById('e2e-tab').getBoundingClientRect().bottom - 1`),
    "the tab menu is under the tab",
  );
  await page.eval(`document.getElementById('e2e-tab')?.removeAttribute('id')`);
  await closeMenu(page);

  // The rail's "+": left click is New project, right click lists it and Open shared project.
  await rightClick(page, ".project-add");
  const add = await page.eval(lines);
  assert.deepEqual(add, ["New project…", "Open shared project…"]);
  await closeMenu(page);

  // The New project dialog has the X, and Esc/X both close it.
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog .modal-close') !== null`);
  await page.eval(`document.querySelector('.project-dialog .modal-close').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') === null`);

  // Project menu: look, then sharing, then Delete.
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await page.type("Menu test");
  await page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.project-square[title="Menu test"]') !== null`);
  await rightClick(page, `.project-square[title="Menu test"]`);
  const project = await page.eval(lines);
  assert.equal(project[0], "Rename…");
  assert.equal(project[1], "Color…");
  assert.equal(project[2], "---", `separator after the look items: ${project}`);
  assert.ok(project.includes("Share project…") && project.includes("Export connections…") && project.includes("Open shared project…"), `${project}`);
  assert.deepEqual(project.slice(-2), ["---", "Delete project"], `Delete last, after a separator: ${project}`);
  await closeMenu(page);
}
