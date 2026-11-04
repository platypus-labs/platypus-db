// The longest context menus are grouped in submenus, as the table's: the connection's (23 lines
// before), the schema's (18) and the SQL editor's (14). Every action is still there, a submenu
// away.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { submenuItems } from "../menu-sub.mjs";
import { connectWith, needsPostgres, pgConnection, treeRow, visibleTab } from "../qa-jrn.mjs";

export const skip = needsPostgres;
const NAME = "S fix menus";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000ff401", NAME)}`);
}

/** The first level of the open context menu: its lines' labels, without the shortcuts. */
const firstLevel = `[...document.querySelector('.context-menu:not(.submenu)').children].filter(e => e.tagName !== 'HR').map(e => (e.querySelector('button') ?? e).querySelector('span')?.textContent ?? e.textContent)`;

async function openMenu(page, expr) {
  await page.eval(`(() => { const el = ${expr}; el.scrollIntoView({ block: 'center' }); const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.x + Math.min(30, r.width / 2), clientY: r.y + r.height / 2 })); })()`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(firstLevel);
}

async function close(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

/** The menu at `expr` has at most `max` lines, the submenus `groups` with these items. */
async function check(page, what, expr, max, groups) {
  const lines = await openMenu(page, expr);
  assert.ok(lines.length <= max, `${what}: ${lines.length} lines: ${lines.join(" | ")}`);
  for (const [name, items] of Object.entries(groups)) {
    assert.ok(lines.includes(name), `${what}: no ${name} submenu in ${lines.join(" | ")}`);
    const inside = await submenuItems(page, name);
    for (const item of items) assert.ok(inside.includes(item), `${what} › ${name}: no ${item} in ${inside.join(" | ")}`);
  }
  await close(page);
}

export default async function (page) {
  await connectWith(page, NAME);
  await page.waitFor(`${treeRow("public")} !== undefined`, 20_000);

  await check(page, "connection", treeRow(NAME), 12, {
    New: ["New connection…", "New saved script", "New notebook", "New dashboard"],
    Open: ["Open notebook…", "Dashboards…", "Run SQL file…"],
    Tools: ["Server activity…", "Back up…", "Restore…", "Schedule a task…", "Transfer profiles…", "Export documentation…"],
    Export: ["Export connections…", "Copy as URL"],
  });
  await check(page, "schema", treeRow("public"), 10, {
    Data: ["Import data into a new table…", "Paste CSV as a new table…", "Export all tables…", "Dump to SQL file…", "Copy schema to a database…", "Back up schema…"],
    Analyze: ["Check schema…", "Compare with…", "Schema history", "Export documentation…"],
  });

  await page.eval(`${treeRow(NAME)}.id = 's-fix-menus'`);
  await page.clickSel("#s-fix-menus", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select 1");
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === 'select 1'`);
  await check(page, "editor", `${visibleTab}.querySelector('.cm-line')`, 11, {
    Run: ["Run in a new result tab", "Run on connections…"],
    Explain: ["Explain plan", "Explain analyze (runs the statement)"],
  });

  // An action in a submenu runs: Explain plan opens the plan of the statement.
  await openMenu(page, `${visibleTab}.querySelector('.cm-line')`);
  await submenuItems(page, "Explain");
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  await page.eval(`[...document.querySelectorAll('.context-menu.submenu button')].find(b => b.textContent.startsWith('Explain plan')).click()`);
  await page.waitFor(`document.querySelectorAll('.tab').length > ${tabs}`, 15_000);
}
