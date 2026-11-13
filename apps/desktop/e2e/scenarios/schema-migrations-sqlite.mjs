// Migration status of a project folder against a throwaway SQLite database: a Flyway project
// (applied, edited since applied, pending, pending but older than the latest applied, failed,
// recorded with no file, repeatable) and a Rails one, the folder picked with the system dialog
// and remembered, the status counts, the "Only problems" filter, a missing folder, an empty
// folder, and a double click on a `.sql` migration opening it in the editor. A second database
// has the history tables of Liquibase (XML, YAML and formatted SQL changelogs), Prisma (applied,
// edited, failed, rolled back, pending, recorded with no file) and Alembic (a chain with a
// branch), each shown in its own section with its own statuses.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { answerSave, menu as menuAt } from "../transfer-helpers.mjs";

const NAME = "Migrations db";
const NAME2 = "Migrations db 2";
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

  seedOtherTools(dir);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1
[[connections]]
id = "00000000-0000-4000-8000-000000000041"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "app.db")}'
save_password = false

[[connections]]
id = "00000000-0000-4000-8000-000000000042"
name = "${NAME2}"
driver = "sqlite"
database = '${join(dir, "app2.db")}'
save_password = false
`,
  );
}

const OTHER = {
  "changelog/db.changelog.xml": `<?xml version="1.0" encoding="UTF-8"?>
<databaseChangeLog xmlns="http://www.liquibase.org/xml/ns/dbchangelog">
  <changeSet id="1" author="ann"><createTable tableName="a"><column name="id" type="int"/></createTable></changeSet>
  <changeSet id="2" author="ann"><addColumn tableName="a"><column name="b" type="int"/></addColumn></changeSet>
  <changeSet id="3" author="bob"><createTable tableName="c"><column name="id" type="int"/></createTable></changeSet>
</databaseChangeLog>
`,
  "changelog/extra.sql": "--liquibase formatted sql\n--changeset carl:10\nCREATE TABLE d (id INTEGER);\n",
  "changelog/more.yaml": "databaseChangeLog:\n  - changeSet:\n      id: y1\n      author: dana\n      changes: []\n",
  "prisma/migrations/migration_lock.toml": 'provider = "sqlite"\n',
  "prisma/migrations/20260101000000_init/migration.sql": "CREATE TABLE users (id INTEGER PRIMARY KEY);\n",
  "prisma/migrations/20260102000000_add_posts/migration.sql": "CREATE TABLE posts (id INTEGER PRIMARY KEY); -- edited\n",
  "prisma/migrations/20260103000000_failed/migration.sql": "CREATE TABLE oops (;\n",
  "prisma/migrations/20260104000000_rolled/migration.sql": "CREATE TABLE rolled (id INTEGER);\n",
  "prisma/migrations/20260105000000_new/migration.sql": "CREATE TABLE fresh (id INTEGER);\n",
  "alembic/versions/aaa111_create.py": '"""create tables"""\nrevision = "aaa111"\ndown_revision = None\n',
  "alembic/versions/bbb222_add_col.py": '"""add a column"""\nrevision: str = "bbb222"\ndown_revision: str = "aaa111"\n',
  "alembic/versions/ccc333_more.py": '"""more"""\nrevision = "ccc333"\ndown_revision = "bbb222"\n',
  "alembic/versions/ddd444_branch.py": '"""a branch"""\nrevision = "ddd444"\ndown_revision = "aaa111"\n',
};
const sha256 = (text) => createHash("sha256").update(text).digest("hex");

/** A second project and database: Liquibase, Prisma and Alembic. */
function seedOtherTools(dir) {
  const project = join(dir, "project2");
  for (const [rel, text] of Object.entries(OTHER)) {
    const path = join(project, rel);
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, text);
  }
  const db = new DatabaseSync(join(dir, "app2.db"));
  db.exec(`CREATE TABLE DATABASECHANGELOG (ID TEXT, AUTHOR TEXT, FILENAME TEXT, DATEEXECUTED TEXT, ORDEREXECUTED INTEGER, EXECTYPE TEXT);
    CREATE TABLE _prisma_migrations (id TEXT, checksum TEXT, finished_at TEXT, migration_name TEXT, rolled_back_at TEXT);
    CREATE TABLE alembic_version (version_num TEXT PRIMARY KEY);`);
  const lb = db.prepare("INSERT INTO DATABASECHANGELOG VALUES (?, ?, ?, ?, ?, ?)");
  lb.run("1", "ann", "classpath:changelog/db.changelog.xml", "2026-02-01 10:00:00", 1, "EXECUTED");
  lb.run("2", "ann", "changelog/db.changelog.xml", "2026-02-01 10:00:01", 2, "RERAN");
  // Recorded, and its changelog is gone.
  lb.run("9", "zed", "changelog/old.xml", "2026-02-01 10:00:02", 3, "EXECUTED");
  const pr = db.prepare("INSERT INTO _prisma_migrations VALUES (?, ?, ?, ?, ?)");
  const migration = (name) => OTHER[`prisma/migrations/${name}/migration.sql`];
  pr.run("p1", sha256(migration("20260101000000_init")), "2026-03-01 10:00:00", "20260101000000_init", null);
  // Applied with the text before the edit.
  pr.run("p2", sha256("CREATE TABLE posts (id INTEGER PRIMARY KEY);\n"), "2026-03-02 10:00:00", "20260102000000_add_posts", null);
  // Started and never finished.
  pr.run("p3", sha256(migration("20260103000000_failed")), null, "20260103000000_failed", null);
  pr.run("p4", sha256(migration("20260104000000_rolled")), "2026-03-04 10:00:00", "20260104000000_rolled", "2026-03-04 11:00:00");
  pr.run("p0", "abc", "2025-12-01 10:00:00", "20250101000000_ghost", null);
  db.prepare("INSERT INTO alembic_version VALUES ('bbb222')").run();
  db.close();
}

/** Liquibase, Prisma and Alembic against the second database. */
async function otherTools(page, dir) {
  const project = join(dir, "project2");
  await page.eval(`${row(NAME2)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row(NAME2)}.getAttribute('aria-expanded') === 'true'`, 15_000);
  await menuAt(page, "main", "Migration status", dialog, NAME2);
  await page.waitFor(`${dialog} !== null`);
  await page.eval(fill(field("folder"), project));
  await page.waitFor(`${field("folder")}.value === ${JSON.stringify(project)}`);
  await check(page);
  await page.waitFor(`${dialog}.querySelectorAll('section[data-tool]').length === 3`, 15_000);
  assert.deepEqual((await page.eval(sections)).sort(), ["alembic", "liquibase", "prisma"], "Flyway and Rails have no history here");

  // Liquibase: the changeset is told apart by id, author and file, whatever prefix the database
  // wrote before the file name; XML, YAML and formatted SQL changelogs are all read.
  const liquibase = { 1: "applied", 2: "applied", 3: "pending", 9: "missing", 10: "pending", y1: "pending" };
  for (const [id, status] of Object.entries(liquibase)) {
    assert.equal(await page.eval(statusOf("liquibase", id)), status, `Liquibase ${id}`);
  }
  assert.equal(await page.eval(`${dialog}.querySelectorAll('section[data-tool="liquibase"] tbody tr').length`), 6);
  assert.match((await page.eval(cells("liquibase", "2")))[5], /run again/i);
  assert.match((await page.eval(cells("liquibase", "9")))[5], /no file in the folder/);
  assert.match((await page.eval(cells("liquibase", "10")))[2], /extra\.sql$/);
  assert.match((await page.eval(cells("liquibase", "y1")))[2], /more\.yaml$/);
  assert.match(await page.eval(`${dialog}.querySelector('section[data-tool="liquibase"] h3').textContent`), /DATABASECHANGELOG/);

  // Prisma: the SHA-256 of the file is the checksum; a started and unfinished run failed; a
  // rolled back run is pending again.
  const prisma = {
    "20250101000000_ghost": "missing",
    "20260101000000_init": "applied",
    "20260102000000_add_posts": "divergent",
    "20260103000000_failed": "failed",
    "20260104000000_rolled": "pending",
    "20260105000000_new": "pending",
  };
  for (const [id, status] of Object.entries(prisma)) {
    assert.equal(await page.eval(statusOf("prisma", id)), status, `Prisma ${id}`);
  }
  assert.match((await page.eval(cells("prisma", "20260102000000_add_posts")))[5], /checksum differs/);
  assert.match((await page.eval(cells("prisma", "20260104000000_rolled")))[5], /rolled back/);

  // Alembic: the head in the database and all it follows ran; the files after it are pending
  // and, two of them being heads, say so.
  const alembic = { aaa111: "applied", bbb222: "applied", ccc333: "pending", ddd444: "pending" };
  for (const [id, status] of Object.entries(alembic)) {
    assert.equal(await page.eval(statusOf("alembic", id)), status, `Alembic ${id}`);
  }
  assert.match((await page.eval(cells("alembic", "bbb222")))[5], /head/);
  assert.match((await page.eval(cells("alembic", "ccc333")))[5], /branch head/);
  assert.match((await page.eval(cells("alembic", "ddd444")))[5], /branch head/);
  assert.equal((await page.eval(cells("alembic", "aaa111")))[1], "create tables");

  // The counts over the three tools.
  const count = (s) => `${dialog}.querySelector('[data-count="${s}"]').textContent.replace(/\\s+/g, ' ').trim()`;
  assert.equal(await page.eval(count("applied")), "Applied 5");
  assert.equal(await page.eval(count("pending")), "Pending 7");
  assert.equal(await page.eval(count("divergent")), "Divergent 1");
  assert.equal(await page.eval(count("failed")), "Failed 1");
  assert.equal(await page.eval(count("missing")), "Missing 2");
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Close').click()`);
  await page.waitFor(`${dialog} === null`);
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

  await otherTools(page, dir);
}
