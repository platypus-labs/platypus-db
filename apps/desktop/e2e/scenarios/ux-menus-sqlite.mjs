// Group T (UX): the navigator's table menu groups the SQL generators in a "Generate SQL"
// submenu and the data transfers in "Data", with the destructive items (Truncate, Drop) last;
// a right click where no app menu exists never shows the browser's own menu.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "UX db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menu = `document.querySelector('.context-menu')`;
/** The labels of the menu's own lines, in order (a submenu's items are not among them). */
const topLabels = `[...${menu}.children].map(c => (c.matches('button') ? c : c.querySelector(':scope > button'))?.querySelector('span')?.textContent).filter(Boolean)`;
const submenuLabels = `[...document.querySelectorAll('.context-menu.submenu button span:first-child')].map(s => s.textContent)`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ux.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000071"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "ux.db")}'
save_password = false
`,
  );
}

async function openTableMenu(page) {
  await page.eval(`${row("t")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`${menu} !== null`);
}

/** Dispatches a cancelable contextmenu event on the elements matching `selector`; whether the
 *  page cancelled it (the browser's menu would not show). */
const cancelled = (page, selector) =>
  page.eval(
    `(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return null;
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
      el.dispatchEvent(e); return e.defaultPrevented; })()`,
  );

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("t")} !== undefined`, 15_000);

  // The table menu: main actions first, the families in submenus, destructive items last.
  await openTableMenu(page);
  const top = await page.eval(topLabels);
  for (const label of ["Open data", "Generate SQL", "Data", "Truncate…", "Drop…"]) assert.ok(top.includes(label), `${label} in ${top}`);
  for (const gone of ["Generate SELECT", "Generate INSERT", "Generate DDL", "Import data…", "Export data…", "Dump to SQL file…"]) {
    assert.ok(!top.includes(gone), `${gone} is in a submenu, not in ${top}`);
  }
  assert.ok(top.indexOf("Open data") < top.indexOf("Generate SQL"), `${top}`);
  assert.deepEqual(top.slice(-2), ["Truncate…", "Drop…"], `destructive items last in ${top}`);

  // Opening the submenu lists the generators.
  await page.eval(`[...${menu}.querySelectorAll('button')].find(b => b.getAttribute('aria-haspopup') && b.textContent.startsWith('Generate SQL')).click()`);
  await page.waitFor(`${submenuLabels}.includes('Generate SELECT')`);
  const generators = await page.eval(submenuLabels);
  for (const label of ["Generate SELECT", "Generate INSERT", "Generate UPDATE", "Generate DELETE", "Generate DDL"]) {
    assert.ok(generators.includes(label), `${label} in ${generators}`);
  }
  assert.ok(!generators.includes("Drop…"), `no Drop in the Generate SQL submenu: ${generators}`);

  // An item of the submenu still runs: Generate SELECT opens the statement in an editor.
  await page.eval(`[...document.querySelectorAll('.context-menu.submenu button')].find(b => b.textContent.startsWith('Generate SELECT')).click()`);
  await page.waitFor(`/SELECT[\\s\\S]*FROM t\\b/.test([...(${visibleTab}?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join(' '))`, 15_000);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // Escape closes a menu with its submenu open.
  await openTableMenu(page);
  await page.eval(`[...${menu}.querySelectorAll('button')].find(b => b.getAttribute('aria-haspopup') && b.textContent.startsWith('Data')).click()`);
  await page.waitFor(`${submenuLabels}.includes('Import data…')`);
  assert.ok((await page.eval(submenuLabels)).includes("Export data…"));
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // The browser's menu never shows where the app has none; a text field keeps it (cut, copy,
  // paste) and the code editor has its own menu.
  assert.equal(await cancelled(page, "body"), true, "contextmenu on the body is cancelled");
  assert.equal(await cancelled(page, "#root"), true, "contextmenu on the app root is cancelled");
  assert.equal(await page.eval(`document.querySelector('.context-menu')`), null, "no app menu opened there");
  const field = await cancelled(page, "input[type=text], input:not([type]), textarea");
  if (field !== null) assert.equal(field, false, "a text field keeps the native menu");

  // App menus still open after all that.
  await openTableMenu(page);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}
