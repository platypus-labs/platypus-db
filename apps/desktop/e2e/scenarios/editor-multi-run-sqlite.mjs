// Group D, running one script on several connections, on four throwaway SQLite databases:
// a query on all of them comes together in a new tab, with the connection of each row and
// the columns one of them lacks NULL; a connection without the table fails alone; a script
// that changes data asks first, then each connection asks its own Safety question (UPDATE
// without WHERE), the read-only one is skipped and the others changed; parameters are
// refused before anything runs.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { revealItem } from "../menu-sub.mjs";

const EU = "Multi EU";
const US = "Multi US";
const BROKEN = "Multi broken";
const RO = "Multi RO";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].filter(l => !l.closest('.sql-line')).map(l => l.textContent).join('\\n')`;
const dialog = `document.querySelector('.modal.multi-run')`;
const button = (scope, label) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)})`;
/** Each chosen connection's state and text, by name. */
const outcomes = `Object.fromEntries([...${dialog}.querySelectorAll('.multi-run-row.outcome')].map(r => [r.querySelector('.multi-run-name').textContent, r.dataset.state + ': ' + r.querySelector('.multi-run-outcome').textContent]))`;
const settled = `${dialog} !== null && ${dialog}.querySelectorAll('.multi-run-row.outcome').length > 0 && [...${dialog}.querySelectorAll('.multi-run-row.outcome')].every(r => !['queued', 'running'].includes(r.dataset.state)) && ${dialog}.querySelector('.notice:not(.error)') !== null`;

export function setup({ dir }) {
  const make = (file, sql) => {
    const db = new DatabaseSync(join(dir, file));
    db.exec(sql);
    db.close();
  };
  make("eu.db", "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO customers VALUES (1, 'ada'), (2, 'bob');");
  make("us.db", "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, country TEXT); INSERT INTO customers VALUES (7, 'cy', 'US');");
  make("broken.db", "CREATE TABLE other (x INTEGER);");
  make("ro.db", "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO customers VALUES (9, 'dee');");
  const connection = (n, name, file, extra = "") =>
    `[[connections]]\nid = "00000000-0000-4000-8000-0000000d000${n}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, file)}'\nsave_password = false\n${extra}`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n\n${connection(1, EU, "eu.db")}\n${connection(2, US, "us.db")}\n${connection(3, BROKEN, "broken.db")}\n${connection(4, RO, "ro.db", "read_only = true\n")}`,
  );
}

async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
}

/** Opens "Run on connections…" from the editor's menu and chooses every SQLite connection. */
async function openDialog(page, all = true) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.eval(`document.getElementById('e2e-editor').removeAttribute('id')`);
  await page.click(r.x + 40, r.y + 10, { button: "right" });
  await revealItem(page, "Run on connections");
  const item = `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Run on connections'))`;
  await page.waitFor(`${item} !== undefined`);
  await page.eval(`${item}.click()`);
  await page.waitFor(`${dialog} !== null`);
  if (all) {
    await page.eval(`${button(dialog, "All SQLite")}.click()`);
    await page.waitFor(`${dialog}.querySelectorAll('.multi-run-list input:checked').length === 4`);
  }
}

async function copyAll(page) {
  await page.eval(`window.__copied = undefined`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

export default async function (page, { dir, shots }) {
  const shot = async (name) => shots && (await page.screenshot(join(shots, `multi-run-${name}.png`)));
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  const row = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(EU)})`;
  await page.waitFor(`${row} !== undefined`);
  await page.eval(`${row}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-connection').removeAttribute('id')`);
  await page.waitFor(`${visibleTab} !== undefined`);

  // The tab's connection is chosen at first; the filter narrows the list.
  await setEditor(page, "SELECT * FROM customers ORDER BY id");
  await openDialog(page, false);
  assert.deepEqual(
    await page.eval(`[...${dialog}.querySelectorAll('.multi-run-list input:checked')].map(i => i.closest('label').querySelector('.multi-run-name').textContent)`),
    [EU],
  );
  await page.eval(`(() => {
    const input = ${dialog}.querySelector('.multi-run-filter');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'broken');
    input.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.waitFor(`${dialog}.querySelectorAll('.multi-run-list label').length === 1`);
  await page.eval(`(() => {
    const input = ${dialog}.querySelector('.multi-run-filter');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, '');
    input.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.eval(`${button(dialog, "All SQLite")}.click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.multi-run-list input:checked').length === 4`);
  await shot("choose");

  // A query on all four: three answer, the one without the table fails alone.
  await page.eval(`${dialog}.querySelector('[data-action=run]').click()`);
  await page.waitFor(settled, 30_000);
  const read = await page.eval(outcomes);
  assert.deepEqual(read[EU], "done: 1 statement · 2 rows");
  assert.deepEqual(read[US], "done: 1 statement · 1 row");
  assert.deepEqual(read[RO], "done: 1 statement · 1 row", "reading is allowed on a read-only connection");
  assert.match(read[BROKEN], /^failed: Statement 1: .*no such table: customers/);
  await shot("outcomes");
  await page.eval(`${button(dialog, "Close")}.click()`);
  await page.waitFor(`${dialog} === null`);

  // Together in a new tab: the connection first, `country` NULL where a database lacks it.
  await page.waitFor(`${status} === '4 rows'`, 30_000);
  assert.match(await page.eval(editorText), /UNION ALL BY NAME/);
  // UNION ALL keeps no order between the connections: compared sorted.
  const merged = (await copyAll(page)).split("\n").sort();
  assert.deepEqual(merged, [`${EU}\t1\tada\t`, `${EU}\t2\tbob\t`, `${RO}\t9\tdee\t`, `${US}\t7\tcy\tUS`]);
  await shot("merged");
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`${editorText} === 'SELECT * FROM customers ORDER BY id'`);

  // A change asks first; the read-only connection is skipped, the others changed.
  await setEditor(page, "UPDATE customers SET name = upper(name)");
  await openDialog(page);
  await page.eval(`${dialog}.querySelector('[data-action=run]').click()`);
  const confirm = `document.querySelector('.modal.confirm')`;
  await page.waitFor(`${confirm} !== null`);
  assert.match(await page.eval(`${confirm}.textContent`), /may change data on 4 connections/);
  await page.eval(`${button(confirm, "Run")}.click()`);
  // Without WHERE, each connection asks on its own, one at a time, naming itself: the broken
  // one is declined. The read-only one is skipped before asking.
  const guard = `document.querySelector('.modal.guard')`;
  const asked = [];
  for (let i = 0; i < 3; i++) {
    await page.waitFor(`${guard} !== null`, 30_000);
    const title = await page.eval(`${guard}.querySelector('h2').textContent`);
    asked.push(title);
    await page.eval(`${button(guard, title.includes(BROKEN) ? "Cancel" : "Run")}.click()`);
    await page.waitFor(`${guard} === null || ${guard}.querySelector('h2').textContent !== ${JSON.stringify(title)}`);
  }
  assert.deepEqual(asked.sort(), [BROKEN, EU, US].map((n) => `Change every row on ${n}?`).sort());
  await page.waitFor(settled, 30_000);
  const wrote = await page.eval(outcomes);
  assert.equal(wrote[EU], "done: 1 statement · 2 changed");
  assert.equal(wrote[US], "done: 1 statement · 1 changed");
  assert.equal(wrote[RO], "skipped: Read-only connection");
  assert.equal(wrote[BROKEN], "skipped: Not confirmed");
  assert.match(await page.eval(`${dialog}.querySelector('.notice').textContent`), /No connection returned rows/);
  await page.eval(`${button(dialog, "Close")}.click()`);
  await page.waitFor(`${dialog} === null`);
  const names = (file) => {
    const db = new DatabaseSync(join(dir, file));
    const out = db.prepare("SELECT name FROM customers ORDER BY id").all().map((r) => r.name);
    db.close();
    return out;
  };
  assert.deepEqual(names("eu.db"), ["ADA", "BOB"]);
  assert.deepEqual(names("us.db"), ["CY"]);
  assert.deepEqual(names("ro.db"), ["dee"]);

  // Declining the question runs nothing.
  await setEditor(page, "DELETE FROM customers");
  await openDialog(page);
  await page.eval(`${dialog}.querySelector('[data-action=run]').click()`);
  await page.waitFor(`${confirm} !== null`);
  await page.eval(`${button(confirm, "Cancel")}.click()`);
  await page.waitFor(`${confirm} === null`);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.multi-run-row.outcome').length`), 0);
  await page.eval(`${button(dialog, "Close")}.click()`);
  assert.deepEqual(names("eu.db"), ["ADA", "BOB"]);

  // Parameters cannot be filled in on several connections.
  await setEditor(page, "SELECT * FROM customers WHERE id = :id");
  await openDialog(page);
  await page.eval(`${dialog}.querySelector('[data-action=run]').click()`);
  await page.waitFor(`${dialog}.querySelector('.notice.error')?.textContent.includes('Parameters') === true`);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.multi-run-row.outcome').length`), 0);
  await page.eval(`${button(dialog, "Close")}.click()`);
}
