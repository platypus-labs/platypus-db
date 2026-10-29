// Migration status of a project folder against a throwaway SQLite database: a Flyway project
// (applied, edited since applied, pending, pending but older than the latest applied, failed,
// recorded with no file, repeatable) and a Rails one, the folder picked with the system dialog
// and remembered, the status counts, the "Only problems" filter, a missing folder, an empty
// folder, and a double click on a `.sql` migration opening it in the editor.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { answerSave } from "../qa-xfer.mjs";

const NAME = "Migrations db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const dialog = `document.querySelector('.modal.schema-migrations')`;
const editorText = `[...document.querySelectorAll('.cm-content')].map(e => e.textContent).join('\\n')`;
const migration = (tool, id) => `${dialog}.querySelector('section[data-tool="${tool}"] tr[data-id="${id}"]')`;
const statusOf = (tool, id) => `${migration(tool, id)}?.dataset.status`;
const cells = (tool, id) => `[...${migration(tool, id)}.querySelectorAll('td')].map(td => td.textContent)`;
const sections = `[...${dialog}.querySelectorAll('section[data-tool]')].map(s => s.dataset.tool)`;
const field = (name) => `${dialog}.querySelector('[data-field="${name}"]')`;

/** Sets the React-controlled input `el` to `value`, as typing would. */
const fill = (el, value) => `(() => {
  const el = ${el};
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;

/** Flyway's checksum: the CRC32 of the lines without their terminators (a leading byte order
 *  mark dropped), read as a signed 32-bit number. Written out here so the scenario does not
 *  depend on the Node version's `zlib.crc32`. */
function flywayChecksum(text) {
  const bytes = Buffer.from(text.replace(/^﻿/, "").replace(/[\r\n]/g, ""), "utf8");
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let k = 0; k < 8; k++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (crc ^ 0xffffffff) | 0;
}

const FILES = {
  "db/migration/V1__create_users.sql": "CREATE TABLE users (id INTEGER PRIMARY KEY);\n",
  "db/migration/V1_5__late_fix.sql": "CREATE INDEX users_id ON users (id);\n",
  // The database has the checksum of the text before this edit.
  "db/migration/V2__add_email.sql": "ALTER TABLE users ADD email TEXT; -- edited after it ran\r\n",
  "db/migration/V3__add_orders.sql": "CREATE TABLE orders (id INTEGER PRIMARY KEY);\n",
  "db/migration/V4__broken.sql": "CREATE TABLE broken (;\n",
  "db/migration/R__user_view.sql": "CREATE VIEW user_ids AS SELECT id FROM users;\n",
  "rails/db/migrate/20260101000000_create_users.rb": "class CreateUsers < ActiveRecord::Migration[7.1]; end\n",
  "rails/db/migrate/20260102000000_add_email.rb": "class AddEmail < ActiveRecord::Migration[7.1]; end\n",
  "rails/db/migrate/20260103000000_add_orders.rb": "class AddOrders < ActiveRecord::Migration[7.1]; end\n",
  // Not migrations: dependencies and build output are skipped.
  "node_modules/pkg/V7__ignored.sql": "SELECT 1;\n",
  "target/V8__ignored.sql": "SELECT 1;\n",
};

export function setup({ dir }) {
  const project = join(dir, "project");
  for (const [rel, text] of Object.entries(FILES)) {
    const path = join(project, rel);
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, text);
  }
  mkdirSync(join(dir, "empty"), { recursive: true });

  const sum = (rel) => flywayChecksum(FILES[`db/migration/${rel}`]);
  const db = new DatabaseSync(join(dir, "app.db"));
  db.exec(`CREATE TABLE flyway_schema_history (
    installed_rank INTEGER PRIMARY KEY, version TEXT, description TEXT NOT NULL, type TEXT NOT NULL, script TEXT NOT NULL,
    checksum INTEGER, installed_by TEXT NOT NULL, installed_on TEXT NOT NULL, execution_time INTEGER NOT NULL, success INTEGER NOT NULL);
    CREATE TABLE schema_migrations (version TEXT PRIMARY KEY);`);
  const insert = db.prepare("INSERT INTO flyway_schema_history VALUES (?, ?, ?, 'SQL', ?, ?, 'app', ?, 5, ?)");
  insert.run(1, "1", "create users", "V1__create_users.sql", sum("V1__create_users.sql"), "2026-01-01 10:00:00", 1);
  // V2 ran with another text than the file has now: divergent.
  insert.run(2, "2", "add email", "V2__add_email.sql", flywayChecksum("ALTER TABLE users ADD email TEXT;"), "2026-01-02 10:00:00", 1);
  insert.run(3, "4", "broken", "V4__broken.sql", sum("V4__broken.sql"), "2026-01-03 10:00:00", 0);
  // Recorded, and the file is gone.
  insert.run(4, "2.5", "ghost", "V2_5__ghost.sql", 123, "2026-01-04 10:00:00", 1);
  // The repeatable migration ran with the text it has.
  db.prepare("INSERT INTO flyway_schema_history VALUES (5, NULL, 'user view', 'SQL', 'R__user_view.sql', ?, 'app', '2026-01-05 10:00:00', 5, 1)").run(
    flywayChecksum(FILES["db/migration/R__user_view.sql"]),
  );
  for (const v of ["20260101000000", "20260102000000", "20250101000000"]) db.prepare("INSERT INTO schema_migrations VALUES (?)").run(v);
  db.close();

  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1
[[connections]]
id = "00000000-0000-4000-8000-000000000041"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "app.db")}'
save_password = false
`,
  );
}

/** Runs the context-menu entry starting with `title` on the navigator row `label`. */
async function menu(page, label, title) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(title)})).click()`,
  );
}

/** Clicks Check and waits for the result of this run: the button is disabled while it runs. */
async function check(page) {
  await page.eval(`${dialog}.querySelector('[data-action="check"]').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-action="check"]').disabled === false`, 20_000);
}

export default async function (page, { dir, shots }) {
  const project = join(dir, "project");
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);

  await menu(page, "main", "Migration status");
  await page.waitFor(`${dialog} !== null`);
  // Nothing remembered yet: no folder, nothing to check.
  assert.equal(await page.eval(`${field("folder")}.value`), "");
  assert.equal(await page.eval(`${dialog}.querySelector('[data-action="check"]').disabled`), true);

  // A folder that does not exist: the error shows inline, no table.
  await page.eval(fill(field("folder"), join(dir, "nope")));
  await page.waitFor(`${dialog}.querySelector('[data-action="check"]').disabled === false`);
  await check(page);
  await page.waitFor(`${field("error")} !== null`, 10_000);
  assert.match(await page.eval(`${field("error")}.textContent`), /does not exist/);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('section[data-tool]').length`), 0);

  // A folder without any migration: the history tables are in the database, so each tool shows
  // what it recorded, and none of it has a file.
  await page.eval(fill(field("folder"), join(dir, "empty")));
  await page.waitFor(`${field("folder")}.value === ${JSON.stringify(join(dir, "empty"))}`);
  await check(page);
  await page.waitFor(`${dialog}.querySelectorAll('section[data-tool]').length === 2`, 10_000);
  assert.equal(await page.eval(`${field("error")} === null`), true, "the error goes with the next check");
  assert.deepEqual(await page.eval(sections), ["flyway", "rails"]);
  assert.equal(await page.eval(`${migration("flyway", "1")}.dataset.status`), "missing");
  assert.equal(await page.eval(`${migration("flyway", "4")}.dataset.status`), "failed");
  assert.equal(await page.eval(`${migration("rails", "20260101000000")}.dataset.status`), "missing");
  assert.equal(
    await page.eval(`${dialog}.querySelectorAll('tr[data-status="applied"], tr[data-status="pending"]').length`),
    0,
    "nothing in the folder is applied or pending",
  );

  // The project folder, picked with the system dialog (answered here).
  await answerSave(page, project, "plugin:dialog|open");
  await page.eval(`${dialog}.querySelector('[data-action="browse"]').click()`);
  await page.waitFor(`${field("folder")}.value === ${JSON.stringify(project)}`, 10_000);
  await check(page);
  await page.waitFor(`${statusOf("flyway", "1")} === 'applied'`, 10_000);

  // Flyway: every status, with the checksum pair and the out of order mark. Rails apart.
  assert.deepEqual(await page.eval(sections), ["flyway", "rails"]);
  const expected = {
    1: "applied",
    "1.5": "pending",
    2: "divergent",
    3: "pending",
    4: "failed",
    "2.5": "missing",
    R: "applied",
  };
  for (const [id, status] of Object.entries(expected)) {
    assert.equal(await page.eval(statusOf("flyway", id)), status, `Flyway ${id}`);
  }
  assert.equal(await page.eval(`${dialog}.querySelectorAll('section[data-tool="flyway"] tbody tr').length`), 7);
  const late = await page.eval(cells("flyway", "1.5"));
  assert.match(late[2], /V1_5__late_fix\.sql$/);
  assert.match(late[5], /out of order/);
  const edited = await page.eval(cells("flyway", "2"));
  assert.equal(edited[1], "add email");
  assert.match(edited[5], /checksum differs \(database -?\d+, file -?\d+\)/);
  assert.equal(edited[4], "2026-01-02 10:00:00", "when it was applied");
  assert.equal((await page.eval(cells("flyway", "3")))[5], "", "a plain pending migration has no detail");
  assert.equal((await page.eval(cells("flyway", "2.5")))[2], "", "recorded, no file");
  assert.match((await page.eval(cells("flyway", "2.5")))[5], /no file in the folder/);
  assert.match(await page.eval(`${dialog}.querySelector('section[data-tool="flyway"] h3').textContent`), /flyway_schema_history/);
  assert.equal(await page.eval(`${migration("flyway", "R")}.querySelector('td').textContent`), "R");

  // Rails.
  const rails = { 20250101000000: "missing", 20260101000000: "applied", 20260102000000: "applied", 20260103000000: "pending" };
  for (const [id, status] of Object.entries(rails)) {
    assert.equal(await page.eval(statusOf("rails", id)), status, `Rails ${id}`);
  }
  assert.equal((await page.eval(cells("rails", "20260103000000")))[1], "add orders");

  // The counts over both tools.
  const count = (s) => `${dialog}.querySelector('[data-count="${s}"]').textContent.replace(/\\s+/g, ' ').trim()`;
  assert.equal(await page.eval(count("applied")), "Applied 4");
  assert.equal(await page.eval(count("pending")), "Pending 3");
  assert.equal(await page.eval(count("divergent")), "Divergent 1");
  assert.equal(await page.eval(count("failed")), "Failed 1");
  assert.equal(await page.eval(count("missing")), "Missing 2");
  if (shots) await page.screenshot(join(shots, "migrations.png"));

  // Only problems hides the applied ones, and shows them again.
  await page.eval(`${field("only-problems")}.click()`);
  await page.waitFor(`${dialog}.querySelectorAll('tr[data-status="applied"]').length === 0`);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('tbody tr').length`), 7);
  await page.eval(`${field("only-problems")}.click()`);
  await page.waitFor(`${dialog}.querySelectorAll('tr[data-status="applied"]').length === 4`);

  // A Rails migration is a Ruby file: a double click does nothing.
  await page.eval(`${migration("rails", "20260103000000")}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  assert.equal(await page.eval(`${dialog} !== null`), true);
  assert.equal(await page.eval(`${field("error")} === null`), true);

  // A SQL migration opens in the editor, and the dialog closes.
  await page.eval(`${migration("flyway", "3")}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${dialog} === null`, 10_000);
  await page.waitFor(`(${editorText}).includes('CREATE TABLE orders (id INTEGER PRIMARY KEY)')`, 15_000);

  // The folder is remembered for the connection, and checked as the dialog opens.
  await menu(page, "main", "Migration status");
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(`${field("folder")}.value`), project);
  await page.waitFor(`${statusOf("flyway", "2")} === 'divergent'`, 15_000);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Close').click()`);
  await page.waitFor(`${dialog} === null`);
}
