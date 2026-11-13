// Sensitive columns reach agents masked: run_query and sample_rows read `••••••`, NULL stays
// NULL, and an alias or a function around the column changes nothing. In Settings > Agents >
// Security the user can show them on one connection, and only there; the choice is approved by
// being made in the app, while the same line written in the file by hand gets the agent nothing.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";
import { fill } from "../transfer-helpers.mjs";
import { runQuery, status, treeRow } from "../query.mjs";

const PEOPLE = "00000000-0000-4000-8000-0000000000e1";
const OTHER = "00000000-0000-4000-8000-0000000000e2";
const EMAIL = "ann@example.com";
const SECRET = "owes 20 euros";

const row = (name) => `document.querySelector('.agents-panel tr[data-connection=${JSON.stringify(name)}]')`;
const shown = `document.querySelector('.agent-detail [data-field="agent_values"]')`;

export const skip = () => noMcp();

export function setup({ dir }) {
  const entry = (id, name, file) =>
    `\n[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, file)}'\nsave_password = false\n`;
  for (const file of ["people.db", "other.db"]) {
    const db = new DatabaseSync(join(dir, file));
    db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
             INSERT INTO people VALUES (1, 'Ann', '${EMAIL}'), (2, 'Bob', NULL);
             CREATE TABLE vault (id INTEGER PRIMARY KEY, note TEXT);
             INSERT INTO vault VALUES (1, '${SECRET}'), (2, NULL);
             CREATE TABLE plain (id INTEGER PRIMARY KEY, note TEXT);
             INSERT INTO plain VALUES (1, '${SECRET}'), (2, NULL);`);
    db.close();
  }
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${entry(PEOPLE, "People", "people.db")}${entry(OTHER, "Other", "other.db")}`);
}

async function chooseInSecurity(page, name) {
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`, 10_000);
  await page.eval(`document.querySelector('.modal.settings [role="tab"][data-section="Agents"]').click()`);
  await page.waitFor(`document.querySelector('[data-agents-page="security"]') !== null`, 10_000);
  await page.eval(`document.querySelector('[data-agents-page="security"]').click()`);
  await page.waitFor(`${row(name)} !== null`);
  await page.eval(`${row(name)}.click()`);
  await page.waitFor(`${shown} !== null`);
}

const dialog = `document.querySelector('.agent-context')`;

/** Opens a SQL tab on the connection `name` by a double click on its row. */
async function openTab(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`[...document.querySelectorAll('.query-tab')].some(t => t.style.display !== 'none' && t.querySelector('.cm-content') != null)`, 15_000);
  await page.eval(`document.getElementById('e2e-conn')?.removeAttribute('id')`);
}

/** What the user shares of the open tab, once, as get_user_context returns it. */
async function sharedContext(page, mcp) {
  const pending = mcp.call("get_user_context");
  await page.waitFor(`${dialog} !== null`, 20_000);
  await page.eval(`${dialog}.querySelector('[data-answer=once]').click()`);
  const r = await pending;
  assert.equal(r.error, false, r.text);
  await page.waitFor(`${dialog} === null`);
  return r;
}

/** The user's own tab: a column read under another name is masked in what they share too. */
async function sharedAlias(page, dir, mcp) {
  const file = join(dir, "agent-bridge.json");
  for (let i = 0; i < 100 && !existsSync(file); i++) await page.sleep(100);
  assert.ok(existsSync(file), "the app wrote agent-bridge.json");
  await openTab(page, "People");
  await runQuery(page, "SELECT id, email AS contact FROM people ORDER BY id");
  const r = await sharedContext(page, mcp);
  assert.ok(!r.text.includes(EMAIL), r.text);
  const context = JSON.parse(r.text);
  assert.deepEqual(context.result.columns, ["id", "contact"]);
  assert.deepEqual(context.result.rows, [["1", "••••••"], ["2", null]]);
  assert.ok(context.notes.some((n) => /masked: contact/.test(n)), r.text);
  // A statement that hides the column behind a subquery is masked whole, not shown.
  await runQuery(page, "SELECT c FROM (SELECT email AS c FROM people) ORDER BY c");
  const nested = await sharedContext(page, mcp);
  assert.ok(!nested.text.includes(EMAIL), nested.text);
  // One that names no sensitive column is left as it is.
  await runQuery(page, "SELECT name AS who FROM people ORDER BY id");
  const plain = JSON.parse((await sharedContext(page, mcp)).text);
  assert.deepEqual(plain.result.rows, [["Ann"], ["Bob"]]);
}

const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const noteSelect = `${objectTab}?.querySelector('select[aria-label="Sensitive: note"]')`;

/** Opens the details of table `table` of People and marks its `note` column sensitive. */
async function markNote(page, table) {
  await page.eval(`${treeRow(table)}.querySelector('.tree-label').id = 'e2e-row'`);
  const r = await page.rect("#e2e-row");
  await page.eval(`document.getElementById('e2e-row').removeAttribute('id')`);
  await page.click(r.x + 5, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Open details')).click()`);
  await page.waitFor(`${objectTab} !== undefined`, 10_000);
  await page.eval(`[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Columns')).click()`);
  await page.waitFor(`${noteSelect} != null`, 10_000);
  await page.eval(fill(noteSelect, "yes"));
  await page.waitFor(`${noteSelect}.value === 'yes'`, 5000);
}

/** Opens the data viewer from the open details tab and waits for its two rows. */
async function openData(page) {
  await page.eval(`[...${objectTab}.querySelectorAll('button')].find(b => b.textContent === 'Open data').click()`);
  await page.waitFor(`${status} === '2 rows'`, 15_000);
}

/** A data viewer tab tells the server its table: `vault.note` is marked and masked, `plain.note`
 *  (same name, unmarked) is shared, also after the viewer's statement was changed by hand; a free
 *  query in an editor tab on `plain` is read by its statement. */
async function sharedViewer(page, mcp) {
  await page.waitFor(`${treeRow("People")} !== undefined`);
  // The double click that opened the SQL tab may have expanded the connection already.
  if (!(await page.eval(`${treeRow("vault")} !== undefined`))) {
    await page.eval(`${treeRow("People")}.querySelector(".twisty").click()`);
  }
  await page.waitFor(`${treeRow("vault")} !== undefined && ${treeRow("plain")} !== undefined`, 15_000);
  await markNote(page, "vault");
  await openData(page);
  const vault = JSON.parse((await sharedContext(page, mcp)).text);
  assert.ok(!JSON.stringify(vault).includes(SECRET), "the marked column is masked in the viewer");
  assert.deepEqual(vault.result.rows, [["1", "••••••"], ["2", null]]);
  // The viewer's statement changed in the editor still reads that table.
  await runQuery(page, "SELECT note FROM vault WHERE id = 1");
  const filtered = await sharedContext(page, mcp);
  assert.ok(!filtered.text.includes(SECRET), filtered.text);
  // The table without a mark: the same column name is shared.
  await page.eval(`${treeRow("plain")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  await page.eval(`document.getElementById('e2e-table')?.removeAttribute('id')`);
  const plain = JSON.parse((await sharedContext(page, mcp)).text);
  assert.deepEqual(plain.result.rows, [["1", SECRET], ["2", null]]);
  // An editor tab with a free query reads its statement.
  await openTab(page, "People");
  await runQuery(page, "SELECT id, note FROM plain ORDER BY id");
  const free = JSON.parse((await sharedContext(page, mcp)).text);
  assert.deepEqual(free.result.rows, [["1", SECRET], ["2", null]]);
  await runQuery(page, "SELECT id, note FROM vault ORDER BY id");
  const freeVault = await sharedContext(page, mcp);
  assert.ok(!freeVault.text.includes(SECRET), freeVault.text);
}

/** The JSON result of `sql` on connection `name`. */
async function read(mcp, name, sql) {
  const r = await mcp.call("run_query", { connection: name, sql, format: "json" });
  assert.equal(r.error, false, r.text);
  return JSON.parse(r.text);
}

export default async function (page, { dir }) {
  const mcp = await startMcp(dir, "e2e-masking");
  try {
    // By default, on every way of asking for the column.
    const sample = await mcp.call("sample_rows", { connection: "People", schema: "main", table: "people" });
    assert.ok(!sample.text.includes(EMAIL), sample.text);
    assert.match(sample.text, /••••••/);
    assert.match(sample.text, /Sensitive columns are masked: email\./);
    for (const sql of [
      "SELECT email FROM people",
      "SELECT email AS contact FROM people",
      "SELECT upper(email) FROM people",
      "SELECT *, email AS contact FROM people",
    ]) {
      const text = (await mcp.call("run_query", { connection: "People", sql })).text;
      assert.ok(!text.toLowerCase().includes(EMAIL), `${sql}: ${text}`);
    }
    const masked = await read(mcp, "People", "SELECT id, name, email FROM people ORDER BY id");
    assert.deepEqual(masked.rows, [[1, "Ann", "••••••"], [2, "Bob", null]]);
    assert.deepEqual(masked.masked_columns, ["email"]);

    await sharedAlias(page, dir, mcp);
    await sharedViewer(page, mcp);

    // Shown on one connection from the app: only there, and agents read them at once.
    await chooseInSecurity(page, "People");
    assert.equal(await page.eval(`${shown}.checked`), false);
    await page.eval(`${shown}.click()`);
    await page.waitFor(`${shown}.checked === true`);
    const connections = () => readFileSync(join(dir, "connections.toml"), "utf8");
    assert.match(connections(), /agent_values = "shown"/);
    const open = await read(mcp, "People", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(open.rows, [[EMAIL], [null]]);
    assert.equal(open.masked_columns, undefined);
    const other = await read(mcp, "Other", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(other.rows, [["••••••"], [null]]);

    // The same line written by hand in the file for the other connection gets nothing.
    writeFileSync(
      join(dir, "connections.toml"),
      connections().replace(`id = "${OTHER}"`, `id = "${OTHER}"\nagent_values = "shown"`),
    );
    const edited = await read(mcp, "Other", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(edited.rows, [["••••••"], [null]], "an edit outside the app waits for the user");
    writeFileSync(
      join(dir, "connections.toml"),
      connections().replace(`id = "${OTHER}"\nagent_values = "shown"`, `id = "${OTHER}"`),
    );

    // Masked again from the app.
    await page.eval(`${shown}.click()`);
    await page.waitFor(`${shown}.checked === false`);
    assert.doesNotMatch(connections(), /agent_values/);
    const again = await read(mcp, "People", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(again.rows, [["••••••"], [null]]);
  } finally {
    await mcp.close?.();
  }
}
