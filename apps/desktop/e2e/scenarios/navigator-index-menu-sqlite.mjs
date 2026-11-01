// An index row of the navigator has a context menu, like the other object rows: Open details
// shows its table, Edit table… opens the table editor, Copy name is there, and Escape closes it.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Index menu db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const items = `[...document.querySelectorAll('.context-menu button')].map(b => b.querySelector('span').textContent)`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "index-menu.db"));
  db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, email TEXT, city TEXT);
           CREATE UNIQUE INDEX people_email ON people (email);
           CREATE INDEX people_city ON people (city);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000012"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "index-menu.db")}'
save_password = false
`,
  );
}

async function rightClick(page, label) {
  await page.eval(`${row(label)}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 40, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("people")} !== undefined`, 15_000);
  await page.eval(`${row("people")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("Indexes")} !== undefined`, 15_000);
  await page.eval(`${row("Indexes")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("people_email")} !== undefined && ${row("people_city")} !== undefined`);

  // The menu of an index lists its actions.
  await rightClick(page, "people_email");
  const listed = await page.eval(items);
  for (const item of ["Copy name", "Open details", "Edit table…"]) assert.ok(listed.includes(item), `${item} in ${listed}`);
  // It is the index's menu, not the table's: nothing that changes the table's rows.
  assert.ok(!listed.includes("Drop…") && !listed.includes("Truncate…"), `no object actions in ${listed}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // Open details opens the tab of the index's table.
  await rightClick(page, "people_city");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Open details').click()`);
  await page.waitFor(`document.querySelector('.object-tab .object-title strong')?.textContent === 'people'`, 15_000);

  // Edit table… opens the table editor on the same table.
  await rightClick(page, "people_email");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Edit table…').click()`);
  await page.waitFor(`document.querySelector('.table-editor') !== null`, 15_000);
}
