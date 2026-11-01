// Dump to SQL file and Run SQL file, on SQLite: a schema with a foreign key, a key to a later
// row of the same table and a view is dumped (plain and gzip, data or structure only, one
// object), the dump runs in an empty database and gives back the same rows; a failing file
// rolls back in one transaction and keeps what ran without; a production connection asks
// first.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";
import { DatabaseSync } from "node:sqlite";
import { revealItem } from "../menu-sub.mjs";

const rows = (label) =>
  `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const row = (label) => `${rows(label)}[0]`;
const modal = `document.querySelector('.modal.sql-file')`;
const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const guard = `document.querySelector('.modal.guard')`;

export function setup({ dir }) {
  const src = new DatabaseSync(join(dir, "src.db"));
  src.exec(`CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
    CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id), next_id INTEGER REFERENCES child(id), note TEXT);
    INSERT INTO parent VALUES (1, 'it''s'), (2, 'b');
    INSERT INTO child VALUES (1, 1, 2, 'a;b'), (2, 2, NULL, NULL);
    CREATE VIEW named AS SELECT c.id, p.name FROM child c JOIN parent p ON p.id = c.parent_id;`);
  src.close();
  new DatabaseSync(join(dir, "copy.db")).close();
  new DatabaseSync(join(dir, "prod.db")).close();
  writeFileSync(
    join(dir, "bad.sql"),
    "CREATE TABLE made (id INTEGER);\nINSERT INTO made VALUES (1);\n\nINSERT INTO missing VALUES (1);\nCREATE TABLE after (id INTEGER);\n",
  );
  const c = (id, name, file, extra = "") => `
[[connections]]
id = "00000000-0000-4000-8000-00000000071${id}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false
${extra}
`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${c(1, "Source db", "src.db")}${c(2, "Copy db", "copy.db")}${c(3, "Prod db", "prod.db", 'connection_type = "prod"')}`,
  );
}

/** Opens the context menu of the navigator row `label` and picks `item`. */
async function menu(page, label, item) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, item);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`,
  );
  await page.waitFor(`${modal} !== null`);
}

/** Picks `path` with Choose file…, then clicks `action` (enabled once there is a path). */
async function chooseAndRun(page, path, action) {
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(path)})`);
  await page.eval(`${button(modal, "Choose file…")}.click()`);
  await page.waitFor(
    `${modal}.querySelector('input[aria-label="File"]').value === ${JSON.stringify(path)}`,
  );
  await page.waitFor(`!${button(modal, action)}.disabled`);
  await page.eval(`${button(modal, action)}.click()`);
}

async function close(page) {
  await page.waitFor(
    `(${button(modal, "Close")} ?? ${button(modal, "Cancel")}) !== undefined`,
  );
  await page.eval(
    `(${button(modal, "Close")} ?? ${button(modal, "Cancel")}).click()`,
  );
  await page.waitFor(`${modal} === null`);
}

async function setContents(page, value) {
  await page.eval(`(() => {
    const el = ${modal}.querySelector('select');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
}

const query = (dir, file, sql) => {
  const db = new DatabaseSync(join(dir, file));
  try {
    return db
      .prepare(sql)
      .all()
      .map((r) => ({ ...r }));
  } finally {
    db.close();
  }
};
const tableNames = (dir, file) =>
  query(
    dir,
    file,
    "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name",
  ).map((r) => r.name);

async function dumps(page, dir) {
  await page.eval(`${row("Source db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);

  // The whole schema with its rows.
  const plain = join(dir, "dump.sql");
  await menu(page, "main", "Dump to SQL file");
  assert.match(
    await page.eval(`${modal}.querySelector('h2').textContent`),
    /Dump main to an SQL file/,
  );
  await chooseAndRun(page, plain, "Dump");
  await page.waitFor(`${modal}.textContent.includes('Dumped 4 rows.')`, 20_000);
  await close(page);
  const sql = readFileSync(plain, "utf8");
  assert.match(sql, /PRAGMA defer_foreign_keys = ON/);
  assert.ok(
    sql.indexOf("CREATE TABLE") < sql.indexOf("INSERT INTO"),
    "the tables before their rows",
  );
  assert.ok(
    sql.indexOf("parent") < sql.indexOf("child"),
    "a table after those it refers to",
  );
  assert.match(sql, /CREATE VIEW/);

  // Structure only, compressed by its name.
  const gz = join(dir, "ddl.sql.gz");
  await menu(page, "main", "Dump to SQL file");
  await setContents(page, "ddl");
  await chooseAndRun(page, gz, "Dump");
  await page.waitFor(`${modal}.textContent.includes('Dumped 0 rows.')`, 20_000);
  await close(page);
  const ddl = gunzipSync(readFileSync(gz)).toString("utf8");
  assert.match(ddl, /CREATE TABLE/);
  assert.doesNotMatch(ddl, /INSERT INTO/);

  // One object of the navigator: that table alone.
  await page.waitFor(`${row("parent")} !== undefined`, 15_000);
  const one = join(dir, "parent.sql");
  await menu(page, "parent", "Dump to SQL file");
  assert.match(
    await page.eval(`${modal}.querySelector('h2').textContent`),
    /Dump 1 object to an SQL file/,
  );
  await chooseAndRun(page, one, "Dump");
  await page.waitFor(`${modal}.textContent.includes('Dumped 2 rows.')`, 20_000);
  await close(page);
  const only = readFileSync(one, "utf8");
  assert.match(only, /parent/);
  assert.doesNotMatch(only, /child|VIEW/);
  return plain;
}

async function runs(page, dir, plain) {
  // The dump runs in an empty database and gives the same rows back.
  await menu(page, "Copy db", "Run SQL file");
  await chooseAndRun(page, plain, "Run");
  await page.waitFor(`${modal}.textContent.includes('Ran ')`, 20_000);
  await close(page);
  assert.deepEqual(tableNames(dir, "copy.db"), ["child", "named", "parent"]);
  assert.deepEqual(query(dir, "copy.db", "SELECT * FROM named ORDER BY id"), [
    { id: 1, name: "it's" },
    { id: 2, name: "b" },
  ]);
  assert.deepEqual(
    query(dir, "copy.db", "SELECT id, next_id, note FROM child ORDER BY id"),
    [
      { id: 1, next_id: 2, note: "a;b" },
      { id: 2, next_id: null, note: null },
    ],
  );

  // A failing statement: in one transaction nothing stays, its number and line are shown.
  const bad = join(dir, "bad.sql");
  await menu(page, "Copy db", "Run SQL file");
  await chooseAndRun(page, bad, "Run");
  await page.waitFor(
    `${modal}.querySelector('.notice.error') !== null`,
    20_000,
  );
  const error = await page.eval(
    `${modal}.querySelector('.notice.error').textContent`,
  );
  assert.match(error, /^Rolled back\. Statement 3 \(line 4\) failed/);
  assert.match(error, /missing/);
  assert.ok(!tableNames(dir, "copy.db").includes("made"), "rolled back");

  // Without a transaction, what ran before stays and nothing after it runs.
  await page.eval(`${modal}.querySelector('input[type=checkbox]').click()`);
  await page.waitFor(
    `${modal}.textContent.includes('keeps what ran before it')`,
  );
  await page.eval(`${button(modal, "Run")}.click()`);
  await page.waitFor(
    `${modal}.querySelector('.notice.error')?.textContent.startsWith('2 statements ran before.')`,
    20_000,
  );
  await close(page);
  assert.deepEqual(query(dir, "copy.db", "SELECT * FROM made"), [{ id: 1 }]);
  assert.ok(!tableNames(dir, "copy.db").includes("after"));

  // A file that is not there.
  await menu(page, "Copy db", "Run SQL file");
  await chooseAndRun(page, join(dir, "nowhere.sql"), "Run");
  await page.waitFor(
    `${modal}.querySelector('.notice.error')?.textContent.includes('cannot read')`,
    20_000,
  );
  await close(page);

  // A production connection asks first; Cancel runs nothing.
  await menu(page, "Prod db", "Run SQL file");
  await chooseAndRun(page, plain, "Run");
  await page.waitFor(`${guard} !== null`);
  assert.match(await page.eval(`${guard}.textContent`), /dump\.sql/);
  await page.eval(`${button(guard, "Cancel")}.click()`);
  await page.waitFor(`${guard} === null`);
  await close(page);
  assert.deepEqual(tableNames(dir, "prod.db"), []);
}

export default async function (page, { dir }) {
  await page.waitFor(`${row("Source db")} !== undefined`);
  const plain = await dumps(page, dir);
  await runs(page, dir, plain);
}
