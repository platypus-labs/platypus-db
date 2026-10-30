// Helpers of the Server activity scenarios (group J): the admin tab, its session rows and
// details, and sessions of the dev databases run in the background from a shell in their
// container.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { DBS, assert, expand, treeMenu } from "./qa-grid.mjs";

export const adminTab = `[...document.querySelectorAll('.admin-tab')].find(t => t.style.display !== 'none')`;

/** The session row whose statement contains `text`. */
export const rowWith = (text) =>
  `[...(${adminTab}?.querySelectorAll('tr.admin-session') ?? [])].find(r => r.querySelector('.admin-query')?.textContent.includes(${JSON.stringify(text)}))`;

/** The session row of `id`. */
export const rowOf = (id) => `${adminTab}?.querySelector('tr.admin-session[data-id="${id}"]')`;

export const adminButton = (label) =>
  `[...${adminTab}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)})`;

/** A field of the selected session's details (`user`, `wait`, `blocked-by`, …). */
export const field = (key) => `${adminTab}.querySelector('.admin-details [data-field="${key}"] dd')?.textContent ?? null`;

/** Connects `name` and opens Server activity on it. */
export async function openAdmin(page, name, db) {
  await expand(page, name, db);
  await treeMenu(page, name, "Server activity");
  await page.waitFor(`${adminTab} !== undefined`, 15_000);
}

/** A session of `db` running `sql` in the background; `exited` resolves to its output. */
export function background(db, sql) {
  const child = spawn("docker", DBS[db].shell, { stdio: ["pipe", "pipe", "pipe"] });
  let output = "";
  child.stdout.on("data", (d) => (output += d));
  child.stderr.on("data", (d) => (output += d));
  const exited = new Promise((resolve) => child.on("exit", () => resolve(output)));
  child.stdin.end(sql);
  return { child, exited };
}

/** Answers the open confirmation with its red button. */
export const confirmDanger = (page) => confirm(page, "danger");

/** The id of the session row whose statement contains `text`. */
export const idOf = (page, text) => page.eval(`${rowWith(text)}.dataset.id`);

/** Selects the session row whose statement contains `text` and waits for its details. */
export async function select(page, text) {
  const id = await idOf(page, text);
  await page.eval(`${rowWith(text)}.click()`);
  await page.waitFor(`${adminTab}.querySelector('.admin-details-title strong')?.textContent === 'Session ${id}'`);
  return id;
}

/** Waits for the session running `text` to be listed as waiting for `blocker`, and selects it. */
export async function waitBlocked(page, text, blocker) {
  await page.waitFor(`${rowWith(text)}?.classList.contains('waiting') ?? false`, 30_000);
  await select(page, text);
  assert.equal(await page.eval(field("blocked-by")), blocker);
}

/** Switches the tab to its `Sessions` or `Locks` section. */
export async function showSection(page, label) {
  await page.eval(`[...${adminTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith(${JSON.stringify(label)})).click()`);
}

/** Opens the Locks view and checks the chain from `holder` to `waiter` and the lock the
 *  waiter waits for, then goes back to the sessions. */
export async function checkChain(page, holder, waiter) {
  await showSection(page, "Locks");
  await page.waitFor(`${adminTab}.querySelector('.admin-chains') !== null`, 15_000);
  assert.equal(await page.eval(`${adminTab}.querySelector('.admin-chains > li > button').dataset.id`), holder);
  assert.equal(await page.eval(`${adminTab}.querySelector('.admin-chains > li > ul button').dataset.id`), waiter);
  await page.waitFor(
    `[...${adminTab}.querySelectorAll('table.admin-locks tbody tr.waiting')].some(r => r.children[0].textContent === ${JSON.stringify(waiter)})`,
    15_000,
  );
  await showSection(page, "Sessions");
  await page.waitFor(`${adminTab}.querySelector('input[type=search]') !== null`);
}

/** The row of the Tables and indexes view named `name`. */
export const objectRow = (name) => `${adminTab}?.querySelector('table.admin-objects tr[data-name="${name}"]')`;

/** The statement row whose text contains `text`. */
const statementRow = (text) =>
  `[...(${adminTab}?.querySelectorAll('table.admin-statements tbody tr') ?? [])].find(r => r.querySelector('.admin-query').textContent.includes(${JSON.stringify(text)}))`;

/** Seconds as the statement list writes them, back to a number. */
const secondsOf = (text) => {
  const m = /^([\d.,]+) (ms|s)$/.exec(text.trim());
  if (m) return Number(m[1].replace(",", "")) / (m[2] === "ms" ? 1000 : 1);
  const long = /^(\d+) min (\d+) s$/.exec(text.trim());
  return long ? Number(long[1]) * 60 + Number(long[2]) : NaN;
};

/** Answers the open question with its main button, or its `button` (`danger`). */
async function confirm(page, button = "primary") {
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.${button}').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
}

/**
 * The statistics views of Server activity on `db`, with table `table` (an indexed column `v`
 * no query reads, so its index is unused) made by the caller and `slow` run beforehand:
 * the costliest statements list it, ordered and filtered, with its details; the tables and
 * indexes list the table and mark the unused index, alone with "worth a look"; each
 * operation of `operations` runs from the table's details; and the first runs again from
 * the navigator's menu of the table, at `path`.
 */
export async function statisticsFlow(page, { db, name, table, slow, path, operations, output }) {
  await openAdmin(page, name, db);
  await showSection(page, "Costliest statements");
  await page.waitFor(`${adminTab}.querySelector('table.admin-statements tbody tr') !== null`, 20_000);

  // The costliest first: total times never grow down the list.
  const totals = await page.eval(`[...${adminTab}.querySelectorAll('table.admin-statements tbody tr')].map(r => r.children[2].textContent)`);
  const seconds = totals.map(secondsOf);
  assert.ok(seconds.every((s, i) => i === 0 || s <= seconds[i - 1] + 1e-9), `ordered by total time: ${totals}`);
  await page.waitFor(`${statementRow(slow)} !== undefined`, 15_000);

  // By calls: the most called first.
  await page.eval(`${adminTab}.querySelector('.admin-filter[data-order="calls"]').click()`);
  const calls = await page.eval(`[...${adminTab}.querySelectorAll('table.admin-statements tbody tr')].map(r => Number(r.children[1].textContent.replace(/\\D/g, '')))`);
  assert.ok(calls.every((c, i) => i === 0 || c <= calls[i - 1]), `ordered by calls: ${calls}`);

  // The filter keeps the statement; its details show it whole, with Explain and Open.
  const search = `${adminTab}.querySelector('input[type=search]')`;
  const setSearch = (text) =>
    page.eval(`(() => { const i = ${search}; Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(i, ${JSON.stringify(text)}); i.dispatchEvent(new Event('input', { bubbles: true })); })()`);
  await setSearch(slow);
  await page.waitFor(`${adminTab}.querySelectorAll('table.admin-statements tbody tr').length >= 1`);
  await page.eval(`${statementRow(slow)}.click()`);
  await page.waitFor(`${adminTab}.querySelector('.admin-details .admin-statement')?.textContent.includes(${JSON.stringify(slow)})`);
  assert.ok(Number((await page.eval(field("calls"))).replace(/\D/g, "")) >= 2, "run twice");
  assert.notEqual(await page.eval(`${adminButton("Explain")}`), undefined);
  await setSearch("no statement is like this one");
  await page.waitFor(`${adminTab}.querySelector('.admin-empty')?.textContent === 'No statements match.'`);
  await setSearch("");

  // Tables and indexes: the table, its unused index marked, the primary key unique.
  await showSection(page, "Tables and indexes");
  await page.waitFor(`${objectRow(table)} !== null`, 20_000);
  const unused = `${objectRow(`${table}_v`)}?.querySelector('[data-issue="unused"]')`;
  await page.waitFor(`${unused} !== null && ${unused} !== undefined`, 10_000);
  assert.ok(
    await page.eval(`[...${adminTab}.querySelectorAll('tr.admin-kind-index')].some(r => r.textContent.includes('unique'))`),
    "the primary key is unique",
  );
  // Only those worth a look: the unused index stays, under its table.
  await page.eval(`${adminTab}.querySelector('.admin-toolbar input[type=checkbox]').click()`);
  await page.waitFor(`${objectRow(`${table}_v`)} !== null`);
  assert.ok(await page.eval(`${objectRow(table)} !== null`), "the table of the unused index shows above it");
  await page.eval(`${adminTab}.querySelector('.admin-toolbar input[type=checkbox]').click()`);

  // Each operation from the table's details: asked, run, said done.
  await page.eval(`${objectRow(table)}.click()`);
  await page.waitFor(`${adminTab}.querySelector('.admin-details[data-object="${table}"]') !== null`);
  const buttons = await page.eval(`[...${adminTab}.querySelectorAll('.admin-details button[data-operation]')].map(b => b.dataset.operation)`);
  assert.deepEqual(buttons, operations);
  for (const operation of operations) {
    await page.waitFor(`${adminTab}.querySelector('.admin-details button[data-operation="${operation}"]')?.disabled === false`, 20_000);
    await page.eval(`${adminTab}.querySelector('.admin-details button[data-operation="${operation}"]').click()`);
    await confirm(page);
    const result = `${adminTab}.querySelector('.admin-details [data-field="maintenance-result"]')`;
    await page.waitFor(`${result}?.classList.contains('ok') ?? false`, 60_000);
    // The numbers are read again: the details keep the table.
    await page.waitFor(`${adminTab}.querySelector('.admin-details[data-object="${table}"]') !== null`, 20_000);
  }

  // From the navigator: Maintenance › the first operation, in a dialog that shows the output.
  const { openPath } = await import("./qa-dbs.mjs");
  await openPath(page, path);
  await treeMenu(page, table, "Maintenance");
  const label = { vacuum: "Vacuum…", analyze: "Analyze…", check: "Check…", optimize: "Optimize…", reindex: "Reindex…" }[operations[0]];
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.startsWith(${JSON.stringify(label)}))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(label)})).click()`);
  await page.waitFor(`document.querySelector('.maintenance-dialog') !== null`);
  await page.eval(`document.querySelector('.maintenance-dialog button.primary').click()`);
  await page.waitFor(`document.querySelector('.maintenance-dialog [data-field="maintenance-result"]') !== null`, 60_000);
  assert.ok(await page.eval(`document.querySelector('.maintenance-dialog .notice').classList.contains('ok')`));
  if (output) {
    assert.match(await page.eval(`document.querySelector('.maintenance-dialog pre')?.textContent ?? ''`), output);
  }
  await page.eval(`document.querySelector('.maintenance-dialog button.primary').click()`);
  await page.waitFor(`document.querySelector('.maintenance-dialog') === null`);
}

/**
 * Back up and Restore of `name`'s database (group J): Back up… writes `file` with the
 * database's tool, counting the tables; a table dropped then comes back with Restore…, asked
 * first; Cancel on the question changes nothing.
 */
export async function backupFlow(page, { db, name, file, table, count, dropTable }) {
  const { answerSave } = await import("./qa-xfer.mjs");
  const dialog = `document.querySelector('.backup-dialog')`;
  const result = `${dialog}?.querySelector('[data-field="result"]')`;
  await expand(page, name, db);
  await treeMenu(page, name, "Back up…");
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(`${dialog}.querySelector('button.primary').disabled`), true, "no file yet");
  await answerSave(page, file);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Choose file…').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-field="path"]').value === ${JSON.stringify(file)}`);
  await page.eval(`${dialog}.querySelector('button.primary').click()`);
  await page.waitFor(`${result} != null`, 120_000);
  assert.ok(await page.eval(`${result}.classList.contains('ok')`), await page.eval(`${result}.textContent`));
  assert.match(await page.eval(`${result}.textContent`), /^Backup written: .+ in .+\.$/);
  assert.ok(existsSync(file), "the backup file");
  assert.ok(readFileSync(file).length > 0);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Close').click()`);
  await page.waitFor(`${dialog} === null`);

  // The table goes, and comes back from the backup.
  dropTable();
  assert.equal(count(), null, "dropped");
  await treeMenu(page, name, "Restore…");
  await page.waitFor(`${dialog}?.dataset.mode === 'restore'`);
  await answerSave(page, file, "plugin:dialog|open");
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Choose file…').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-field="path"]').value === ${JSON.stringify(file)}`);
  // Asked first: Cancel leaves the database as it is.
  await page.eval(`${dialog}.querySelector('button.primary').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.confirm p').textContent`), /Its statements change the database/);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
  assert.equal(count(), null, "not restored yet");
  await page.eval(`${dialog}.querySelector('button.primary').click()`);
  await confirmDanger(page);
  await page.waitFor(`${result} != null`, 120_000);
  assert.ok(await page.eval(`${result}.classList.contains('ok')`), await page.eval(`${result}.textContent`));
  assert.ok(count() > 0, `${table} is back`);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Close').click()`);
  await page.waitFor(`${dialog} === null`);
}

/** Whether `program` runs, and its major version (`pg_dump (PostgreSQL) 17.2` is 17). */
export function toolVersion(program) {
  const r = spawnSync(program, ["--version"], { encoding: "utf8" });
  if (r.status !== 0) return null;
  return Number(/(\d+)\./.exec(r.stdout)?.[1] ?? 0);
}

/** Types `text` into the search box of the admin tab, the way a person would: React reads the
 *  value from the input event. */
export const setSearch = (page, text) =>
  page.eval(
    `(() => { const i = ${adminTab}.querySelector('input[type=search]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(i, ${JSON.stringify(text)}); i.dispatchEvent(new Event('input', { bubbles: true })); })()`,
  );

/** Sets the refresh interval of the admin tab (`0` is manual). */
export const setRefresh = (page, seconds) =>
  page.eval(
    `(() => { const s = ${adminTab}.querySelector('select.admin-interval'); Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(String(seconds))}); s.dispatchEvent(new Event('change', { bubbles: true })); })()`,
  );

/** The row of the Parameters view named `name`. */
export const parameterRow = (name) => `${adminTab}?.querySelector('table.admin-parameters tr[data-name="${name}"]')`;

/** The names of the rows the Parameters view lists. */
export const parameterNames = (page) =>
  page.eval(`[...${adminTab}.querySelectorAll('table.admin-parameters tbody tr')].map(r => r.dataset.name)`);

/** The text of cell `column` (0 name, 1 value, 2 default, 3 source, 4 context) of a parameter's row. */
export const parameterCell = (page, name, column) => page.eval(`${parameterRow(name)}.children[${column}].textContent`);

/** A tile of the dashboard: its figure, as shown, and the tooltip it has when it shows "—". */
export const tile = (id) => `${adminTab}?.querySelector('.dash-tile[data-tile="${id}"]')`;
export const tileValue = (id) => `(${tile(id)}?.querySelector('[data-field="value"]')?.textContent ?? null)`;
export const tileTitle = (id) => `(${tile(id)}?.querySelector('[data-field="value"]')?.title ?? null)`;

/** How many readings the sparkline of tile `id` draws. */
export const sparkPoints = (id) =>
  `(${tile(id)}?.querySelector('svg.dash-spark polyline')?.getAttribute('points') ?? '').split(' ').filter(Boolean).length`;
