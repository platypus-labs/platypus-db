// Creating views and triggers from the forms of the navigator on a throwaway SQLite database:
// the Create submenu (no function, no sequence), the errors of the form, a view created and
// replaced, a trigger created from the schema and from a table, and the SQL opened in the
// editor. What the database holds is read from the file.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import {
  choose,
  create,
  createEnabled,
  createMenu,
  editorText,
  errorIs,
  fill,
  form,
  notice,
  openCreate,
  openTableTrigger,
  options,
  preview,
  previewHas,
  rightClick,
  reveal,
  row,
  tick,
} from "../schema-objects.mjs";

const NAME = "Objects db";
let file;

export function setup({ dir }) {
  file = join(dir, "objects.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, price REAL);
           CREATE TABLE log (line TEXT);
           INSERT INTO items (name, price) VALUES ('pen', 2), ('desk', 250);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000041"
name = "${NAME}"
driver = "sqlite"
database = '${file}'
save_password = false
`,
  );
}

/** What the file holds under `name`: the SQL of the view or trigger, or null. */
function sqlOf(type, name) {
  const db = new DatabaseSync(file);
  try {
    return db.prepare("SELECT sql FROM sqlite_master WHERE type = ? AND name = ?").get(type, name)?.sql ?? null;
  } finally {
    db.close();
  }
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);

  // The Create submenu offers what SQLite has: views and triggers, no functions or sequences.
  const entries = await createMenu(page, "main");
  assert.deepEqual(entries, ["View…", "Trigger…"]);
  await page.eval(`[...document.querySelectorAll('.context-menu.submenu button')].find(b => b.querySelector('span').textContent === 'View…').click()`);
  await page.waitFor(`${form} !== null`);

  // The form says what is missing, one thing at a time, and Create waits for it.
  await errorIs(page, "A view needs a name.");
  assert.equal(await page.eval(createEnabled), false);
  assert.equal(await page.eval(`${form}.querySelector('[name=materialized]')`), null, "SQLite has no materialized views");
  await fill(page, "name", "cheap");
  await errorIs(page, "A view needs a query.");
  assert.equal(await page.eval(createEnabled), false);
  await fill(page, "query", "SELECT id, price FROM items WHERE price < 10;");
  await previewHas(page, "CREATE VIEW main.cheap AS SELECT id, price FROM items WHERE price < 10;");
  assert.equal(await page.eval(notice), "");
  await create(page);
  await reveal(page, "Views", "cheap");
  assert.match(sqlOf("view", "cheap"), /price < 10/);

  // The same name again fails in the database, and the dialog stays open with its message.
  await openCreate(page, "main", "View…");
  await fill(page, "name", "cheap");
  await fill(page, "query", "SELECT id FROM items");
  await previewHas(page, "CREATE VIEW main.cheap AS SELECT id FROM items;");
  assert.ok(!(await page.eval(preview)).includes("DROP VIEW"));
  await page.eval(`${form}.querySelector('button[name=create]').click()`);
  await errorIs(page, "already exists");
  assert.ok(await page.eval(`${form} !== null`));
  assert.match(sqlOf("view", "cheap"), /price/, "the old view is untouched");

  // Replace drops the old view first, in the same run.
  await tick(page, "replace", true);
  await previewHas(page, "DROP VIEW IF EXISTS main.cheap;");
  await previewHas(page, "CREATE VIEW main.cheap AS SELECT id FROM items;");
  await create(page);
  assert.doesNotMatch(sqlOf("view", "cheap"), /price/);

  // A trigger from the schema's menu, opened in the editor instead of run.
  await openCreate(page, "main", "Trigger…");
  await errorIs(page, "A trigger needs a name.");
  assert.deepEqual(await options(page, "timing"), ["BEFORE", "AFTER", "INSTEAD OF"]);
  await page.waitFor(`${form}.querySelector('[name=table]').options.length === 2`, 15_000);
  await choose(page, "table", "items");
  await fill(page, "name", "items_audit");
  await fill(page, "body", "INSERT INTO log VALUES ('new ' || NEW.name);");
  await fill(page, "when", "NEW.price > 100");
  // SQLite triggers fire on one event.
  await tick(page, "event-update", true);
  await errorIs(page, "SQLite triggers fire on one event.");
  await tick(page, "event-update", false);
  await previewHas(page, "CREATE TRIGGER main.items_audit AFTER INSERT ON items FOR EACH ROW WHEN NEW.price > 100");
  assert.match(await page.eval(preview), /END;$/);
  await page.eval(`${form}.querySelector('button[name=open-in-editor]').click()`);
  await page.waitFor(`${form} === null`);
  await page.waitFor(`(${editorText}).includes('CREATE TRIGGER main.items_audit AFTER INSERT ON items')`, 15_000);
  assert.equal(sqlOf("trigger", "items_audit"), null, "Open in editor runs nothing");

  // The same trigger from the table's own menu, with the table chosen, run.
  await openTableTrigger(page, "items");
  await page.waitFor(`${form}.querySelector('[name=table]').value === 'items'`, 15_000);
  await fill(page, "name", "items_audit");
  await fill(page, "body", "INSERT INTO log VALUES ('new ' || NEW.name);");
  await fill(page, "when", "NEW.price > 100");
  await previewHas(page, "CREATE TRIGGER main.items_audit AFTER INSERT ON items");
  await create(page);
  await reveal(page, "Triggers", "items_audit");
  assert.match(sqlOf("trigger", "items_audit"), /WHEN NEW\.price > 100/);

  // It fires for an expensive item only.
  const db = new DatabaseSync(file);
  db.exec("INSERT INTO items (name, price) VALUES ('lamp', 20), ('sofa', 900)");
  assert.deepEqual(
    db.prepare("SELECT line FROM log").all().map((r) => r.line),
    ["new sofa"],
  );
  db.close();

  // A table's menu has the entry, a view's does not.
  await rightClick(page, "cheap");
  const viewMenu = await page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
  assert.ok(!viewMenu.includes("Create trigger…"), `${viewMenu}`);
}
