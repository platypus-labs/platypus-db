// Cassandra and ScyllaDB from the `cassandra` and `scylla` profiles of docker/compose.yml
// (Cassandra on 59042 with the login cassandra / cassandra, ScyllaDB on 59043 without one):
// the connection form (its own labels, Test connection with a right and a wrong login, a closed
// port, TLS against a plain port, the URL mode, ScyllaDB's tile), the navigator (keyspaces,
// tables, the actions CQL has), the data viewer (CQL, no OFFSET, paging with "Fetch all"), the
// editor (typed results, scripts with `;` inside strings and a batch, `USE`, the server's
// errors, a user-defined type, LWT), the questions before TRUNCATE and a write without a
// filter, a read-only connection, and editing in the grid by the whole primary key: the
// preview shows the statements, saving runs them, the undo script puts rows back, and a change
// of a key column, a row without a key, a counter and a value of the wrong type are refused.
// Skipped when Cassandra is not running.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { button, dialog, editConnection, fill, formField, menu, menuItem, newConnection, picker, saveForm, testConnection, urlField } from "../conn-helpers.mjs";
import { CTRL } from "../cdp.mjs";
import { clearStatus, editorText, gridTsv, openEditor, resultError, runScript, runStatement, setEditor, status, treeRow, visibleTab } from "../qa-sql.mjs";
import { runOrGuard } from "../qa-safe-db.mjs";
import { closePreview, deleteRow, discard, editBar, editCell, insertRow, preview, save, shownError, table } from "../search-helpers.mjs";

const PORT = 59042;
const SCYLLA_PORT = 59043;
const KS = "e2e_cass";
const NAME = "E2E Cassandra";
const RO = "E2E Cassandra read-only";
const CASSANDRA = "platypus-db-cassandra-1";

/** Whether something listens on `port`. */
const listens = (port) =>
  new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
    const done = (up) => {
      socket.destroy();
      resolve(up);
    };
    socket.on("connect", () => done(true));
    socket.on("error", () => done(false));
    socket.on("timeout", () => done(false));
  });

export const skip = async () =>
  (await listens(PORT)) ? null : `no Cassandra on port ${PORT} (docker compose -f docker/compose.yml --profile cassandra up -d --wait cassandra)`;

/** Runs CQL beside the app with cqlsh in the Cassandra container; returns what it printed. */
function cql(script) {
  const r = spawnSync("docker", ["exec", "-i", CASSANDRA, "cqlsh", "-u", "cassandra", "-p", "cassandra"], { input: script, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`cqlsh: ${r.stderr || r.stdout}`);
  return r.stdout.replace(/\r/g, "");
}

/** The rows of a query as JSON objects, read with `SELECT JSON` (the keys in the table's order). */
function rowsOf(query) {
  const out = cql(`SELECT JSON ${query.replace(/^SELECT /i, "")};`);
  return out
    .split("\n")
    .filter((l) => l.trim().startsWith("{"))
    .map((l) => JSON.parse(l.trim()));
}

const connection = (n, name, extra = "") => `[[connections]]
id = "00000000-0000-4000-8000-00000000c${String(n).padStart(3, "0")}"
name = "${name}"
driver = "cassandra"
host = "127.0.0.1"
port = ${PORT}
database = "${KS}"
user = "cassandra"
ssl_mode = "disable"
save_password = false
password_command = "echo cassandra"
${extra}`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n\n${connection(1, NAME)}\n${connection(2, RO, "read_only = true\n")}`);
  const paged = Array.from({ length: 2500 }, (_, i) => `INSERT INTO ${KS}.paged (p, n) VALUES (1, ${i + 1});`).join("\n");
  cql(`DROP KEYSPACE IF EXISTS ${KS};
CREATE KEYSPACE ${KS} WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};
CREATE TYPE ${KS}.address (street text, zip int);
CREATE TABLE ${KS}.items (id int, day date, name text, qty int, price decimal, tags set<text>, props map<text, int>, addr frozen<address>, PRIMARY KEY (id, day));
INSERT INTO ${KS}.items (id, day, name, qty, tags, props) VALUES (1, '2024-01-01', 'Alpha', 1, {'a'}, {'x': 1});
INSERT INTO ${KS}.items (id, day, name, qty, tags) VALUES (2, '2024-01-02', 'Beta', 2, {'b', 'c'});
INSERT INTO ${KS}.items (id, day, name, qty, addr) VALUES (3, '2024-01-03', 'Gamma', 3, {street: 'Main', zip: 12345});
CREATE TABLE ${KS}.items2 (id int, day date, name text, qty int, PRIMARY KEY (id, day));
INSERT INTO ${KS}.items2 (id, day, name, qty) VALUES (1, '2024-01-01', 'Alpha', 1);
INSERT INTO ${KS}.items2 (id, day, name, qty) VALUES (2, '2024-01-02', 'Beta', 2);
INSERT INTO ${KS}.items2 (id, day, name, qty) VALUES (3, '2024-01-03', 'Gamma', 3);
CREATE TABLE ${KS}.counters (id int PRIMARY KEY, hits counter);
UPDATE ${KS}.counters SET hits = hits + 5 WHERE id = 1;
CREATE TABLE ${KS}.guard_t (id int PRIMARY KEY, v int);
INSERT INTO ${KS}.guard_t (id, v) VALUES (1, 1);
INSERT INTO ${KS}.guard_t (id, v) VALUES (2, 2);
CREATE TABLE ${KS}.paged (p int, n int, PRIMARY KEY (p, n));
${paged}
CREATE TABLE ${KS}.typed (id int PRIMARY KEY, big varint, d decimal, u uuid, ts timestamp, b blob, l list<int>);
INSERT INTO ${KS}.typed (id, big, d, u, ts, b, l) VALUES (1, 123456789012345678901234567890, 12.50, 123e4567-e89b-12d3-a456-426614174000, '2024-03-09T13:05:09.250Z', 0xdeadbeef, [1, 2]);
`);
}

const field = (page, label) => page.eval(`${formField(label)}.value`);
const cancel = async (page) => {
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && ${picker} === null`);
};

/** Opens a new form for the picker's tile and fills the host fields. */
async function form(page, tile, fields) {
  await newConnection(page, tile);
  for (const [label, value] of Object.entries({ Host: "127.0.0.1", Port: PORT, ...fields })) {
    await page.eval(fill(formField(label), String(value)));
  }
}

/** Opens a new form in URL mode with `url` typed in. */
async function byUrl(page, url) {
  await newConnection(page, "Apache Cassandra");
  await page.eval(`document.querySelector('.connection-dialog [data-field="connect_by_url"]').click()`);
  await page.waitFor(`document.querySelector(${JSON.stringify(urlField)}) !== null`);
  await page.eval(fill(`document.querySelector(${JSON.stringify(urlField)})`, url));
  await page.sleep(500);
}

const tab = (page, name) =>
  page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith(${JSON.stringify(name)})).click()`);

/** The form, the login and the other ways a connection can fail. */
async function connectionForm(page) {
  await newConnection(page, "Apache Cassandra");
  assert.equal(await field(page, "Port"), "9042");
  assert.ok(await page.eval(`${formField("Keyspace (optional)")} !== undefined`), "keyspace field");
  assert.ok(await page.eval(`${formField("Database")} == null`), "no Database field");
  await cancel(page);

  // The right login names the product; a wrong password, an unknown user and no login at all
  // are refused with the server's words, and the password never shows.
  await form(page, "Apache Cassandra", { User: "cassandra", Password: "cassandra" });
  assert.match(await testConnection(page), /^Connected: Cassandra 5\./);
  await page.eval(fill(formField("Password"), "e2e-WRONG-pw-4471"));
  const refused = await testConnection(page);
  assert.doesNotMatch(refused, /^Connected/, "a wrong password connected");
  assert.ok(!refused.includes("e2e-WRONG-pw-4471"), `the password shows: ${refused}`);
  await page.eval(fill(formField("User"), "nobody"));
  await page.eval(fill(formField("Password"), "cassandra"));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await page.eval(fill(formField("User"), ""));
  await page.eval(fill(formField("Password"), ""));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // A closed port is an error with a reason, not a hang.
  await form(page, "Apache Cassandra", { Port: 59299, User: "cassandra", Password: "cassandra" });
  const closed = await testConnection(page);
  assert.doesNotMatch(closed, /^Connected/);
  assert.ok(closed.length > 0, "an empty error");
  await cancel(page);

  // TLS against a port that speaks plain CQL fails; the SSL mode is what turns it on.
  await form(page, "Apache Cassandra", { User: "cassandra", Password: "cassandra" });
  await tab(page, "Network");
  await page.eval(fill(formField("SSL mode"), "require"));
  await tab(page, "General");
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // URL mode: the user, password, host, port and keyspace of the URL; `cassandras://` is TLS.
  await byUrl(page, `cassandra://cassandra:cassandra@127.0.0.1:${PORT}/${KS}`);
  assert.match(await testConnection(page), /^Connected: Cassandra 5\./);
  await page.eval(`document.querySelector('.connection-dialog [data-field="connect_by_host"]').click()`);
  await page.waitFor(`${formField("Host")} !== undefined`);
  assert.equal(await field(page, "Host"), "127.0.0.1");
  assert.equal(await field(page, "Port"), String(PORT));
  assert.equal(await field(page, "User"), "cassandra");
  assert.equal(await field(page, "Keyspace (optional)"), KS);
  await cancel(page);
  await byUrl(page, `cassandras://cassandra:cassandra@127.0.0.1:${PORT}`);
  await page.eval(`document.querySelector('.connection-dialog [data-field="connect_by_host"]').click()`);
  await tab(page, "Network");
  assert.notEqual(await field(page, "SSL mode"), "disable");
  await tab(page, "General");
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // ScyllaDB has its own tile, the same form, and no login in the container.
  if (await listens(SCYLLA_PORT)) {
    await form(page, "ScyllaDB", { Port: SCYLLA_PORT });
    assert.match(await testConnection(page), /^Connected: ScyllaDB /);
    await cancel(page);
  }

  // Saved with a keyspace, edited, and the saved one lists the keyspace's tables.
  await form(page, "Apache Cassandra", { Name: "E2E Cassandra saved", User: "cassandra", Password: "cassandra", "Keyspace (optional)": KS });
  assert.match(await testConnection(page), /^Connected/);
  await saveForm(page);
  await page.waitFor(`${treeRow("E2E Cassandra saved")} !== undefined`, 15_000);
  await editConnection(page, "E2E Cassandra saved");
  assert.equal(await field(page, "Keyspace (optional)"), KS);
  assert.equal(await field(page, "User"), "cassandra");
  assert.equal(await field(page, "Port"), String(PORT));
  await cancel(page);
}

/** The cells of a column of the shown grid. */
const column = (rows, name) => {
  const at = rows[0].indexOf(name);
  assert.ok(at >= 0, `${name} in ${rows[0]}`);
  // The copy quotes the cells that hold quotes.
  return rows.slice(1).map((r) => r[at].replace(/^"|"$/g, "").replace(/""/g, '"'));
};

async function openTable(page, name, rows, timeout = 30_000) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 30_000);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editorText}.startsWith('SELECT *') && ${status} === ${JSON.stringify(rows)}`, timeout);
}

export default async function (page) {
  await connectionForm(page);

  // The navigator: the connection's keyspace opens by itself with its tables.
  await page.waitFor(`${treeRow(NAME)} !== undefined`, 30_000);
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  // The tables show at once; the user-defined types are in the Types folder, closed.
  await page.waitFor(`${treeRow("items")} !== undefined && ${treeRow("Types")} !== undefined`, 40_000);
  await page.eval(`${treeRow("Types")}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("address")} !== undefined`, 15_000);
  await page.eval(`${treeRow("items")}.id = 'e2e-items'`);
  const items = await menu(page, "#e2e-items");
  for (const item of ["Open data", "Generate SELECT", "Generate INSERT", "Generate UPDATE", "Generate DELETE", "Truncate…", "Drop…"]) {
    assert.ok(items.some((i) => i.startsWith(item)), `${item} in ${items}`);
  }
  for (const item of ["Visual query builder", "Rename…"]) assert.ok(!items.some((i) => i.startsWith(item)), `${item} in ${items}`);
  await menuItem(page, "Open data");
  await page.waitFor(`${editorText}.startsWith('SELECT *') && ${status} === '3 rows'`, 30_000);
  // Partition and clustering keys come first, then the other columns by name.
  assert.deepEqual(table(await gridTsv(page))[0].slice(0, 3), ["id", "day", "addr"]);

  // Collections and user-defined types are JSON text, keys first.
  const rows = table(await gridTsv(page));
  assert.equal(column(rows, "tags")[0], '["a"]');
  assert.equal(column(rows, "props")[0], '{"x":1}');
  assert.deepEqual(JSON.parse(column(rows, "addr")[2]), { street: "Main", zip: 12345 });
  assert.equal(column(rows, "day")[0], "2024-01-01");

  // The editor: typed results, scripts with `;` in strings, comments and a batch, USE.
  await openEditor(page, NAME);
  assert.equal(await runStatement(page, `SELECT * FROM ${KS}.typed WHERE id = 1`), "1 row");
  const typed = table(await gridTsv(page));
  assert.equal(column(typed, "big")[0], "123456789012345678901234567890");
  assert.equal(column(typed, "d")[0], "12.50");
  assert.equal(column(typed, "u")[0], "123e4567-e89b-12d3-a456-426614174000");
  assert.equal(column(typed, "l")[0], "[1,2]");
  assert.match(column(typed, "ts")[0], /2024-03-09.13:05:09/);
  assert.match(await runScript(page, `INSERT INTO ${KS}.guard_t (id, v) VALUES (3, 3); -- a ; comment\nINSERT INTO ${KS}.guard_t (id, v) VALUES (4, 4);\nBEGIN BATCH INSERT INTO ${KS}.guard_t (id, v) VALUES (5, 5); INSERT INTO ${KS}.guard_t (id, v) VALUES (6, 6); APPLY BATCH;`), /./);
  assert.deepEqual((rowsOf(`id FROM ${KS}.guard_t`)).map((r) => r.id).sort((a, b) => a - b), [1, 2, 3, 4, 5, 6]);
  assert.equal(await runStatement(page, `USE ${KS}`), "0 rows affected");
  assert.equal(await runStatement(page, "SELECT id FROM guard_t WHERE id = 3"), "1 row");

  // Lightweight transactions report whether they applied.
  assert.equal(await runStatement(page, `INSERT INTO ${KS}.guard_t (id, v) VALUES (3, 30) IF NOT EXISTS`), "1 row");
  assert.equal(column(table(await gridTsv(page)), "[applied]")[0], "false");
  assert.equal(await runStatement(page, `INSERT INTO ${KS}.guard_t (id, v) VALUES (7, 7) IF NOT EXISTS`), "1 row");
  assert.equal(column(table(await gridTsv(page)), "[applied]")[0], "true");

  // The server's errors show with their kind.
  await runStatement(page, "SELEC 1");
  assert.match(await page.eval(resultError), /SyntaxError/);
  await runStatement(page, `SELECT * FROM ${KS}.nope`);
  assert.match(await page.eval(resultError), /Invalid.*nope/i);
  await runStatement(page, `INSERT INTO ${KS}.guard_t (id, v) VALUES ('x', 1)`);
  assert.match(await page.eval(resultError), /Invalid/i);

  // A write that changes every row asks first; Cancel leaves the rows. A write with a key,
  // and every read, run without asking.
  const before = (rowsOf(`id FROM ${KS}.guard_t`)).length;
  for (const statement of [`TRUNCATE ${KS}.guard_t`, `TRUNCATE TABLE ${KS}.guard_t`, `UPDATE ${KS}.guard_t SET v = 0`, `DELETE FROM ${KS}.guard_t`]) {
    const r = await runOrGuard(page, statement);
    assert.match(r, /^guard: /, `no question for ${statement}: ${r}`);
    assert.equal((rowsOf(`id FROM ${KS}.guard_t`)).length, before, `${statement} changed the table`);
  }
  assert.equal(await runStatement(page, `DELETE FROM ${KS}.guard_t WHERE id = 7`), "0 rows affected");
  assert.equal((rowsOf(`id FROM ${KS}.guard_t`)).length, before - 1);

  // Past the first page: 2,500 rows open with the first 1,000, and "Fetch all" brings the rest.
  await setEditor(page, `SELECT n FROM ${KS}.paged WHERE p = 1`);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`/^1,000 rows loaded · more available/.test(${status})`, 30_000);
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  await page.waitFor(`/^2,500 rows/.test(${status}) && !/more available/.test(${status})`, 60_000);
  const paged = column(table(await gridTsv(page)), "n");
  assert.equal(paged.length, 2500);
  assert.equal(paged[0], "1");
  assert.equal(paged[2499], "2500");
  assert.equal(await runStatement(page, `SELECT n FROM ${KS}.paged WHERE p = 1 LIMIT 7`), "7 rows");

  await editing(page);
  await readOnly(page);
}

/** Editing in the grid by the whole primary key. */
async function editing(page) {
  const original = rowsOf(`* FROM ${KS}.items2`);
  await openTable(page, "items2", "3 rows");
  const layout = async () => {
    const [header, ...body] = table(await gridTsv(page));
    return { header, body, col: (n) => header.indexOf(n), row: (id) => body.findIndex((r) => r[0] === String(id)) };
  };
  let g = await layout();

  // An update: the key in the WHERE, the new text quoted; saving runs it; the undo script
  // writes the old values back.
  await editCell(page, g.row(1), g.col("name"), "Alpha's");
  await editCell(page, g.row(1), g.col("qty"), "41");
  assert.match(await page.eval(editBar), /1 row changed/);
  // The statements run one after the other as they are: the preview adds no semicolon.
  assert.equal(await preview(page), `UPDATE ${KS}.items2 SET name = 'Alpha''s', qty = 41 WHERE id = 1 AND day = '2024-01-01'`);
  await save(page, "3 rows");
  const changed = (rowsOf(`* FROM ${KS}.items2`)).find((r) => r.id === 1);
  assert.deepEqual([changed.name, changed.qty], ["Alpha's", 41]);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('button')].some(b => b.textContent.includes('Undo script'))`, 15_000);
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`[...${visibleTab}.querySelectorAll('button')].find(b => b.textContent.includes('Undo script')).click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${tabs}`, 10_000);
  assert.match(await page.eval(editorText), /^UPDATE e2e_cass\.items2 SET name = 'Alpha', qty = 1 WHERE id = 1 AND day = '2024-01-01';/);
  await runScript(page);
  const byId = (a, b) => a.id - b.id;
  assert.deepEqual(rowsOf(`* FROM ${KS}.items2`).sort(byId), original.sort(byId));

  // A delete, an update and an insert in one save run in that order; the new row takes the
  // key from its cells; an empty cell is not written.
  await openTable(page, "items2", "3 rows");
  g = await layout();
  await deleteRow(page, g.row(3));
  await editCell(page, g.row(2), g.col("qty"), "20");
  await insertRow(page);
  g = await layout();
  await editCell(page, 0, g.col("id"), "9");
  await editCell(page, 0, g.col("day"), "2024-09-09");
  await editCell(page, 0, g.col("name"), "Zed");
  assert.match(await page.eval(editBar), /3 rows changed/);
  assert.deepEqual((await preview(page)).split("\n"), [
    `DELETE FROM ${KS}.items2 WHERE id = 3 AND day = '2024-01-03'`,
    `UPDATE ${KS}.items2 SET qty = 20 WHERE id = 2 AND day = '2024-01-02'`,
    `INSERT INTO ${KS}.items2 (id, day, name) VALUES (9, '2024-09-09', 'Zed')`,
  ]);
  await save(page, "3 rows");
  const now = rowsOf(`* FROM ${KS}.items2`);
  assert.equal(now.find((r) => r.id === 3), undefined);
  assert.equal(now.find((r) => r.id === 2).qty, 20);
  assert.equal(now.find((r) => r.id === 9).name, "Zed");

  // A collection is edited as JSON; NULL empties a column.
  await openTable(page, "items", "3 rows");
  g = await layout();
  await editCell(page, g.row(2), g.col("tags"), '["x","y"]');
  await editCell(page, g.row(1), g.col("props"), '{"k":7}');
  await editCell(page, g.row(3), g.col("addr"), '{"street":"Side","zip":9}');
  const text = await preview(page);
  assert.match(text, /SET tags = \{'x', 'y'\} WHERE id = 2/);
  assert.match(text, /SET props = \{'k': 7\} WHERE id = 1/);
  assert.match(text, /SET addr = \{street: 'Side', zip: 9\} WHERE id = 3/);
  await save(page, "3 rows");
  const items = rowsOf(`* FROM ${KS}.items`);
  assert.deepEqual(items.find((r) => r.id === 2).tags.sort(), ["x", "y"]);
  assert.deepEqual(items.find((r) => r.id === 1).props, { k: 7 });
  assert.deepEqual(items.find((r) => r.id === 3).addr, { street: "Side", zip: 9 });

  // Refused before anything runs: a key column, a value of the wrong type, a collection that
  // is not JSON, a row without its key.
  g = await layout();
  await editCell(page, g.row(1), g.col("id"), "5");
  assert.equal(await preview(page), null);
  assert.match(await page.eval(shownError), /primary key column id cannot change/);
  await discard(page);
  g = await layout();
  await editCell(page, g.row(1), g.col("qty"), "many");
  assert.equal(await preview(page), null);
  assert.match(await page.eval(shownError), /invalid integer/);
  await discard(page);
  g = await layout();
  await editCell(page, g.row(1), g.col("tags"), "not json");
  assert.equal(await preview(page), null);
  assert.match(await page.eval(shownError), /invalid JSON/);
  await discard(page);
  await insertRow(page);
  g = await layout();
  await editCell(page, 0, g.col("name"), "No key");
  assert.equal(await preview(page), null);
  assert.match(await page.eval(shownError), /key/i);
  await discard(page);
  assert.equal((rowsOf(`* FROM ${KS}.items`)).length, 3, "a refused change was written");

  // A counter cannot be edited in the grid (it changes by increments).
  await openTable(page, "counters", "1 row");
  g = await layout();
  await editCell(page, 0, g.col("hits"), "9");
  assert.equal(await preview(page), null);
  assert.match(await page.eval(shownError), /counter/);
  await discard(page);
  assert.equal((rowsOf(`* FROM ${KS}.counters`))[0].hits, 5);
  if (await page.eval(`document.querySelector('.sql-preview') !== null`)) await closePreview(page);
}

/** A read-only connection reads, and refuses what could write. */
async function readOnly(page) {
  await openEditor(page, RO);
  const before = rowsOf(`* FROM ${KS}.items2`);
  for (const statement of [
    `INSERT INTO ${KS}.items2 (id, day, name) VALUES (50, '2024-05-05', 'x')`,
    `UPDATE ${KS}.items2 SET qty = 0 WHERE id = 1 AND day = '2024-01-01'`,
    `DELETE FROM ${KS}.items2 WHERE id = 1 AND day = '2024-01-01'`,
    `TRUNCATE ${KS}.items2`,
    `DROP TABLE ${KS}.items2`,
    `CREATE TABLE ${KS}.created (id int PRIMARY KEY)`,
    `BEGIN BATCH INSERT INTO ${KS}.items2 (id, day) VALUES (51, '2024-05-05'); APPLY BATCH`,
  ]) {
    const r = await runOrGuard(page, statement, { readOnly: true });
    assert.match(r, /^read-only: .*read-only/, `${statement}: ${r}`);
  }
  assert.equal(await runStatement(page, `SELECT * FROM ${KS}.items2`), "3 rows");
  assert.equal(await runStatement(page, `SELECT * FROM ${KS}.items2 WHERE id = 1 LIMIT 1`), "1 row");
  assert.deepEqual(rowsOf(`* FROM ${KS}.items2`), before);
  assert.equal(rowsOf(`table_name FROM system_schema.tables WHERE keyspace_name = '${KS}' AND table_name = 'created'`).length, 0);
}
