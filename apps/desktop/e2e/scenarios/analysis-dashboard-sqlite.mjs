// Custom dashboards (group M) on SQLite:
// - a dashboard of widgets from saved scripts: a table (with its row limit), a chart (its
//   settings, shown and hidden) and a single value, each with its own state and Refresh;
// - Refresh all and one widget's Refresh read the data again after it changed;
// - a failing script, a script that may change data (an UPDATE, a DELETE after a SELECT), one
//   with parameters, one with a client command, and one renamed since: each shows its reason
//   in its widget while the others keep working, and no data changes;
// - a widget is edited (script, title, size), moved up and down and removed;
// - the dashboard is saved as a file in the data folder, kept from closing with unsaved
//   changes, reopened from the "Dashboards…" list, replaced only after asking, renamed and
//   deleted from the list;
// - the auto-refresh interval is kept in the file, and refreshes a shown dashboard.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { setField } from "../plans.mjs";
import { sqliteConnection } from "../query.mjs";
import { treeMenu } from "../qa-grid.mjs";

const NAME = "E2E dashboard";
const ID = "00000000-0000-4000-8000-000000000041";

const SCRIPTS = {
  "regions.sql": "SELECT region, sum(amount) AS total FROM sales GROUP BY region ORDER BY region",
  "total.sql": "SELECT sum(amount) AS total FROM sales",
  "broken.sql": "SELECT nope FROM sales",
  "wipe.sql": "UPDATE sales SET amount = 0",
  "mixed.sql": "SELECT 1 AS one; DELETE FROM sales",
  "params.sql": "SELECT * FROM sales WHERE region = :region",
  "command.sql": "@set x = 1",
  "ghost.sql": "SELECT count(*) AS n FROM sales",
};

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "board.db",
    id: ID,
    name: NAME,
    sql: `CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount INTEGER);
           INSERT INTO sales VALUES (1, 'north', 10), (2, 'south', 5), (3, 'north', 7), (4, 'east', 3);`,
  });
  // The scripts are files of the All connections folder, each linked to the connection.
  const folder = join(dir, "scripts", "all");
  mkdirSync(folder, { recursive: true });
  for (const [file, sql] of Object.entries(SCRIPTS)) writeFileSync(join(folder, file), sql);
  writeFileSync(join(folder, ".links.json"), JSON.stringify(Object.fromEntries(Object.keys(SCRIPTS).map((f) => [f, ID]))));
}

const tab = `[...document.querySelectorAll('.dashboard-tab')].find(t => t.style.display !== 'none')`;
const widget = (title) => `${tab}?.querySelector('[data-title=${JSON.stringify(title)}]')`;
const click = (scope, action) => `${scope}.querySelector('[data-action=${JSON.stringify(action)}]').click()`;
/** A widget's action: opens its "⋯" menu and picks the item (the menu closes by itself). */
async function act(page, title, action) {
  await page.eval(click(widget(title), "menu"));
  await page.waitFor(`document.querySelector('.context-menu [data-action="dashboard.${action}"]') != null`);
  await page.eval(`document.querySelector('.context-menu [data-action="dashboard.${action}"]').click()`);
}
/** Whether an item of the widget's menu is greyed out; the menu is closed again after. */
async function itemDisabled(page, title, action) {
  await page.eval(click(widget(title), "menu"));
  await page.waitFor(`document.querySelector('.context-menu [data-action="dashboard.${action}"]') != null`);
  const off = await page.eval(`document.querySelector('.context-menu [data-action="dashboard.${action}"]').getAttribute('aria-disabled') === 'true'`);
  await page.eval(`document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })), window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))`);
  await page.waitFor(`document.querySelector('.context-menu') == null`);
  return off;
}
/** When the widget's last run ended: it changes at every run, an error's included. */
const atOf = (title) => `(${widget(title)}?.querySelector('[data-field=status]')?.dataset.at ?? '')`;
const statusOf = (title) => `(${widget(title)}?.querySelector('[data-field=status]')?.textContent ?? '')`;
const errorOf = (title) => `(${widget(title)}?.querySelector('[data-field=error]')?.textContent ?? '')`;
const valueOf = (title) => `(${widget(title)}?.querySelector('[data-field=value]')?.textContent ?? '')`;
const titles = `[...${tab}.querySelectorAll('.dashboard-widget')].map(w => w.dataset.title)`;
const chart = (title) => `${widget(title)}?.querySelector('.chart-svg')`;
const modal = (cls) => `document.querySelector('.modal${cls}')`;

/** Runs `action` and waits until the widget `title` has ended a new run. */
async function refreshed(page, title, action) {
  const before = await page.eval(atOf(title));
  await action();
  await page.waitFor(`${atOf(title)} !== ${JSON.stringify(before)} && ${atOf(title)} !== ''`, 30_000);
}

/** Fills the widget form (`w`: script, title, view, rows, width, height, connection) and saves it. */
async function fillForm(page, w) {
  await page.waitFor(`document.querySelector('.dashboard-form [data-field=script] option[value=${JSON.stringify(w.script)}]') !== null`, 15_000);
  for (const field of ["script", "title", "connection", "view", "rows", "width", "height"]) {
    if (w[field] !== undefined) await setField(page, `.dashboard-form [data-field=${field}]`, String(w[field]));
  }
  await page.eval(`document.querySelector('.dashboard-form [data-action=save]').click()`);
  await page.waitFor(`document.querySelector('.dashboard-form') === null`);
}

/** Adds a widget and waits for its first run. */
async function addWidget(page, w) {
  const title = w.title;
  const count = await page.eval(`${titles}.length`);
  await page.eval(click(tab, "add"));
  await page.waitFor(`document.querySelector('.dashboard-form') !== null`);
  await fillForm(page, w);
  await page.waitFor(`${titles}.length === ${count + 1} && ${atOf(title)} !== ''`, 30_000);
}

async function newDashboard(page, title) {
  const before = await page.eval(`document.querySelectorAll('.dashboard-tab').length`);
  await treeMenu(page, NAME, "New dashboard");
  await page.waitFor(`document.querySelectorAll('.dashboard-tab').length > ${before} && ${tab} != null`);
  if (title) await setIn(page, "title", title);
}

/** Sets a field of the visible dashboard's toolbar as typing or choosing would. */
const setIn = (page, field, value) =>
  page.eval(`(() => {
    const el = ${tab}.querySelector('[data-field=${field}]');
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  })()`);

/** Changes the data the scripts read. */
function sql(dir, statement) {
  const db = new DatabaseSync(join(dir, "board.db"));
  db.exec("PRAGMA busy_timeout = 5000");
  db.exec(statement);
  db.close();
}

const sum = (dir) => {
  const db = new DatabaseSync(join(dir, "board.db"));
  const row = db.prepare("SELECT sum(amount) AS total, count(*) AS n FROM sales").get();
  db.close();
  return row;
};

/** The three kinds of widget, each with its rows. */
async function widgetsOfEachView(page) {
  assert.equal(await page.eval(`${tab}.querySelector('.dashboard-empty') != null`), true, "an empty dashboard says so");
  await addWidget(page, { script: "regions.sql", title: "Regions", view: "table" });
  assert.match(await page.eval(statusOf("Regions")), /^Updated .*3 rows$/);
  await page.waitFor(`${widget("Regions")}.querySelector('.grid-scroll') != null`);
  assert.equal(await page.eval(`${tab}.querySelector('.dashboard-empty')`), null);

  // The rows shown are the first ones.
  await addWidget(page, { script: "regions.sql", title: "Top 2", view: "table", rows: 2 });
  assert.match(await page.eval(statusOf("Top 2")), /first 2 rows/);

  // A chart: drawn bare from the columns, with its settings open when it was just added.
  await addWidget(page, { script: "regions.sql", title: "Revenue by region", view: "chart", width: 2 });
  await page.waitFor(`${chart("Revenue by region")}?.querySelectorAll('rect.chart-bar').length === 3`, 15_000);
  assert.equal(await page.eval(`${widget("Revenue by region")}.style.gridColumn`), "span 2");
  await page.waitFor(`${widget("Revenue by region")}.querySelector('.chart-side') != null`);
  await setField(page, `[data-title="Revenue by region"] select[aria-label="Chart type"]`, "pie");
  await page.waitFor(`${chart("Revenue by region")}?.querySelectorAll('path.chart-slice').length === 3`);
  await act(page, "Revenue by region", "chart_settings");
  await page.waitFor(`${widget("Revenue by region")}.querySelector('.chart-side') == null`);
  assert.equal(await page.eval(`${chart("Revenue by region")}.querySelectorAll('path.chart-slice').length`), 3, "the pie stays");

  // A single value: the first cell.
  await addWidget(page, { script: "total.sql", title: "Total", view: "value" });
  await page.waitFor(`${valueOf("Total")} === '25'`);
}

/** One widget's Refresh and Refresh all read the data again. */
async function refreshing(page, dir) {
  sql(dir, "INSERT INTO sales VALUES (5, 'west', 100)");
  // Nothing changes by itself.
  assert.equal(await page.eval(valueOf("Total")), "25");
  await refreshed(page, "Total", () => act(page, "Total", "refresh"));
  await page.waitFor(`${valueOf("Total")} === '125'`);
  assert.match(await page.eval(statusOf("Regions")), /3 rows$/, "the other widgets keep their rows");

  const before = await page.eval(`[...${tab}.querySelectorAll('.dashboard-widget')].map(w => w.querySelector('[data-field=status]').dataset.at)`);
  await page.eval(click(tab, "refresh_all"));
  await page.waitFor(
    `(() => { const before = ${JSON.stringify(before)}; const now = [...${tab}.querySelectorAll('.dashboard-widget')].map(w => w.querySelector('[data-field=status]').dataset.at); return now.length === before.length && now.every((at, i) => at !== before[i]); })()`,
    30_000,
  );
  assert.match(await page.eval(statusOf("Regions")), /4 rows$/);
  assert.match(await page.eval(statusOf("Top 2")), /first 2 rows/);
  await page.waitFor(`${chart("Revenue by region")}?.querySelectorAll('path.chart-slice').length === 4`, 15_000);
}

/** Scripts that cannot run show why in their widget, and the others go on. */
async function failures(page, dir) {
  await addWidget(page, { script: "broken.sql", title: "Broken", view: "table" });
  assert.match(await page.eval(errorOf("Broken")), /nope/);

  const refused = [
    ["wipe.sql", "Wipe", /may change data/],
    ["mixed.sql", "Mixed", /may change data/],
    ["params.sql", "Params", /parameters/],
    ["command.sql", "Command", /Client commands/],
  ];
  for (const [script, title, reason] of refused) {
    await addWidget(page, { script, title, view: "table" });
    assert.match(await page.eval(errorOf(title)), reason, title);
  }
  // Nothing was changed by the scripts that were refused.
  assert.deepEqual({ ...sum(dir) }, { total: 125, n: 5 });

  // The others are not affected.
  assert.equal(await page.eval(valueOf("Total")), "125");
  assert.match(await page.eval(statusOf("Regions")), /4 rows$/);
  assert.equal(await page.eval(errorOf("Regions")), "");
  assert.equal(await page.eval(`${tab}.querySelectorAll('[data-field=error]').length`), 5);

  // A script renamed since the widget was made: a clear error, the widget stays.
  await addWidget(page, { script: "ghost.sql", title: "Ghost", view: "value" });
  await page.waitFor(`${valueOf("Ghost")} === '5'`);
  renameSync(join(dir, "scripts", "all", "ghost.sql"), join(dir, "scripts", "all", "ghost-2.sql"));
  await refreshed(page, "Ghost", () => act(page, "Ghost", "refresh"));
  assert.match(await page.eval(errorOf("Ghost")), /ghost\.sql no longer exists/);
  assert.equal(await page.eval(valueOf("Ghost")), "", "no old value beside the error");
  assert.equal(await page.eval(valueOf("Total")), "125");
}

/** Edit, move and remove widgets. */
async function arranging(page) {
  // Edit: another script, a new title and width; the widget runs again on the new script.
  const before = await page.eval(atOf("Broken"));
  await act(page, "Broken", "edit");
  await page.waitFor(`document.querySelector('.dashboard-form') !== null`);
  // The saved scripts arrive after the form opens: until then the choice reads "Loading…".
  await page.waitFor(`document.querySelector('.dashboard-form [data-field=script]').value === "broken.sql"`, 10_000);
  await fillForm(page, { script: "total.sql", title: "Fixed total", view: "value", width: 2, height: 2 });
  await page.waitFor(`${widget("Fixed total")} != null && ${atOf("Fixed total")} !== ${JSON.stringify(before)}`, 30_000);
  await page.waitFor(`${valueOf("Fixed total")} === '125'`);
  assert.equal(await page.eval(errorOf("Fixed total")), "");
  assert.equal(await page.eval(`${widget("Broken")}`), null, "the old title is gone");
  assert.equal(await page.eval(`[${widget("Fixed total")}.style.gridColumn, ${widget("Fixed total")}.style.gridRow].join('|')`), "span 2|span 2");

  // The form lists a missing script, so that editing the widget does not change it.
  await act(page, "Ghost", "edit");
  await page.waitFor(`document.querySelector('.dashboard-form [data-field=script]') != null`);
  await page.waitFor(`document.querySelector('.dashboard-form [data-field=script] option[value="ghost.sql"]') != null`);
  assert.equal(await page.eval(`document.querySelector('.dashboard-form [data-field=script]').value`), "ghost.sql");
  // Pointing it at the renamed script repairs it.
  await fillForm(page, { script: "ghost-2.sql" });
  await page.waitFor(`${valueOf("Ghost")} === '5'`, 30_000);
  assert.equal(await page.eval(errorOf("Ghost")), "");

  // Move: the first cannot go up, the last cannot go down; a move swaps neighbours.
  const order = await page.eval(titles);
  assert.deepEqual(order.slice(0, 3), ["Regions", "Top 2", "Revenue by region"]);
  assert.equal(await itemDisabled(page, "Regions", "up"), true);
  assert.equal(await itemDisabled(page, "Ghost", "down"), true);
  await act(page, "Regions", "down");
  await page.waitFor(`${titles}.slice(0, 2).join() === 'Top 2,Regions'`);
  await act(page, "Regions", "up");
  await page.waitFor(`${titles}.slice(0, 2).join() === 'Regions,Top 2'`);

  // Remove.
  await act(page, "Wipe", "remove");
  await page.waitFor(`${widget("Wipe")} == null`);
  assert.equal((await page.eval(titles)).includes("Mixed"), true);
}

/** Saved as a file, kept from closing unsaved, reopened from the list. */
async function saving(page, dir) {
  const file = join(dir, "dashboards", "all", "Sales board.json");
  assert.equal(existsSync(file), false);
  await setIn(page, "auto_refresh", "5");
  await page.eval(click(tab, "save"));
  for (let i = 0; i < 100 && !existsSync(file); i++) await page.sleep(100);
  assert.ok(existsSync(file), "the file is written");
  const saved = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(saved.format, "platypus-dashboard");
  assert.equal(saved.title, "Sales board");
  assert.equal(saved.refresh, 5);
  assert.equal(saved.connection, ID);
  const names = saved.widgets.map((w) => w.title);
  assert.deepEqual(names.slice(0, 4), ["Regions", "Top 2", "Revenue by region", "Total"]);
  assert.equal(saved.widgets.find((w) => w.title === "Revenue by region").chart.kind, "pie");
  assert.equal(saved.widgets.find((w) => w.title === "Top 2").rows, 2);
  assert.equal(saved.widgets.find((w) => w.title === "Ghost").script, "ghost-2.sql");
  await page.waitFor(`[...document.querySelectorAll('[role=tab]')].some(t => t.textContent.includes('Sales board'))`);

  // A change after saving keeps the tab from closing without asking.
  await act(page, "Mixed", "remove");
  await page.waitFor(`${widget("Mixed")} == null`);
  await page.eval(`document.querySelector('.tab.active button[aria-label="Close tab"]').click()`);
  await page.waitFor(`${modal(".confirm")} != null`);
  assert.match(await page.eval(`${modal(".confirm")}.textContent`), /unsaved changes/);
  await page.eval(`[...${modal(".confirm")}.querySelectorAll('button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`${modal(".confirm")} == null`);
  assert.equal(await page.eval(`${tab} != null`), true, "the tab stays");

  // Saved again under the same name: no question, the file follows.
  await page.eval(click(tab, "save"));
  for (let i = 0; i < 100 && JSON.parse(readFileSync(file, "utf8")).widgets.some((w) => w.title === "Mixed"); i++) await page.sleep(100);
  assert.equal(JSON.parse(readFileSync(file, "utf8")).widgets.some((w) => w.title === "Mixed"), false);
  assert.equal(await page.eval(`${modal(".confirm")}`), null, "same name: no question");

  // Closed now without a question, and opened again from the connection's list.
  await page.eval(`document.querySelector('.tab.active button[aria-label="Close tab"]').click()`);
  await page.waitFor(`document.querySelectorAll('.dashboard-tab').length === 0`);
  await treeMenu(page, NAME, "Dashboards…");
  await page.waitFor(`${modal(".dashboards-dialog")}?.querySelector('[data-dashboard="Sales board"]') != null`);
  await page.eval(click(`${modal(".dashboards-dialog")}.querySelector('[data-dashboard="Sales board"]')`, "open"));
  await page.waitFor(`document.querySelectorAll('.dashboard-tab').length === 1 && ${modal(".dashboards-dialog")} == null`);
  await page.waitFor(`${tab} != null && ${titles}.length === ${saved.widgets.length - 1}`);
  assert.equal(await page.eval(`${tab}.querySelector('[data-field=title]').value`), "Sales board");
  assert.equal(await page.eval(`${tab}.querySelector('[data-field=auto_refresh]').value`), "5");
  // The widgets run on open: the data, the pie, and the reasons of the ones that fail.
  await page.waitFor(`${valueOf("Total")} === '125'`, 30_000);
  await page.waitFor(`${chart("Revenue by region")}?.querySelectorAll('path.chart-slice').length === 4`, 30_000);
  await page.waitFor(`${statusOf("Regions")}.endsWith('4 rows')`, 30_000);
  await page.waitFor(`${errorOf("Params")} !== '' && ${errorOf("Command")} !== ''`, 30_000);
  assert.match(await page.eval(errorOf("Params")), /parameters/);
  await page.waitFor(`${valueOf("Ghost")} === '5'`, 30_000);
  assert.equal(await page.eval(`${widget("Revenue by region")}.querySelector('.chart-side')`), null, "settings closed on open");
  assert.equal(await page.eval(`${widget("Revenue by region")}.style.gridColumn`), "span 2");
  // Opening it from the list again brings the open tab forward instead of making another.
  await treeMenu(page, NAME, "Dashboards…");
  await page.waitFor(`${modal(".dashboards-dialog")}?.querySelector('[data-dashboard="Sales board"]') != null`);
  await page.eval(click(`${modal(".dashboards-dialog")}.querySelector('[data-dashboard="Sales board"]')`, "open"));
  await page.waitFor(`${modal(".dashboards-dialog")} == null`);
  assert.equal(await page.eval(`document.querySelectorAll('.dashboard-tab').length`), 1);
}

/** A name taken is replaced only after asking; rename and delete from the list. */
async function files(page, dir) {
  const file = join(dir, "dashboards", "all", "Sales board.json");
  const size = JSON.parse(readFileSync(file, "utf8")).widgets.length;
  await newDashboard(page, "Sales board");
  await page.eval(click(tab, "save"));
  await page.waitFor(`${modal(".confirm")} != null`);
  assert.match(await page.eval(`${modal(".confirm")}.textContent`), /Sales board.*Replace/s);
  await page.eval(`[...${modal(".confirm")}.querySelectorAll('button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`${modal(".confirm")} == null`);
  await page.sleep(300);
  assert.equal(JSON.parse(readFileSync(file, "utf8")).widgets.length, size, "Cancel leaves the file");
  // Another name: a new file.
  await setIn(page, "title", "Empty board");
  await page.eval(click(tab, "save"));
  const second = join(dir, "dashboards", "all", "Empty board.json");
  for (let i = 0; i < 100 && !existsSync(second); i++) await page.sleep(100);
  assert.equal(JSON.parse(readFileSync(second, "utf8")).widgets.length, 0);

  // Rename and delete from the list.
  await treeMenu(page, NAME, "Dashboards…");
  const dialog = modal(".dashboards-dialog");
  await page.waitFor(`${dialog}?.querySelectorAll('[data-dashboard]').length === 2`);
  await page.eval(click(`${dialog}.querySelector('[data-dashboard="Empty board"]')`, "rename"));
  await page.waitFor(`document.querySelector('.name-dialog input') != null`);
  await setField(page, ".name-dialog input", "Quarterly board");
  await page.eval(`document.querySelector('.name-dialog button[type=submit]').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-dashboard="Quarterly board"]') != null && ${dialog}.querySelector('[data-dashboard="Empty board"]') == null`);
  assert.equal(existsSync(second), false);
  assert.equal(existsSync(join(dir, "dashboards", "all", "Quarterly board.json")), true);
  // A name that is taken is refused, and the dialog says why.
  await page.eval(click(`${dialog}.querySelector('[data-dashboard="Quarterly board"]')`, "rename"));
  await page.waitFor(`document.querySelector('.name-dialog input') != null`);
  await setField(page, ".name-dialog input", "Sales board");
  await page.eval(`document.querySelector('.name-dialog button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.name-dialog .notice.error') != null`);
  await page.eval(`[...document.querySelectorAll('.name-dialog button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.name-dialog') == null`);

  await page.eval(click(`${dialog}.querySelector('[data-dashboard="Quarterly board"]')`, "delete"));
  await page.waitFor(`${modal(".confirm")} != null`);
  await page.eval(`[...${modal(".confirm")}.querySelectorAll('button')].find(b => b.textContent === 'Delete').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-dashboard="Quarterly board"]') == null`);
  assert.equal(existsSync(join(dir, "dashboards", "all", "Quarterly board.json")), false);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('[data-dashboard]').length`), 1);
  await page.eval(`[...${dialog}.querySelectorAll('.actions button')].find(b => b.textContent === 'Close').click()`);
  await page.waitFor(`${dialog} == null`);
}

/** The interval refreshes the dashboard that is shown. */
async function autoRefresh(page, dir) {
  // Back to the "Sales board" tab.
  await page.eval(`[...document.querySelectorAll('[role=tab]')].find(t => t.textContent.includes('Sales board')).click()`);
  await page.waitFor(`${tab}.querySelector('[data-field=title]').value === 'Sales board'`);
  await setIn(page, "auto_refresh", "1");
  sql(dir, "INSERT INTO sales VALUES (6, 'west', 5)");
  assert.equal(await page.eval(valueOf("Total")), "125");
  // A minute is the shortest interval.
  await page.waitFor(`${valueOf("Total")} === '130'`, 120_000);
  await setIn(page, "auto_refresh", "0");
  assert.equal(await page.eval(`${tab}.querySelector('[data-field=auto_refresh]').value`), "0");
}

export default async function (page, { dir }) {
  await newDashboard(page, "Sales board");
  assert.ok((await page.eval(`[...document.querySelectorAll('[role=tab]')].map(t => t.textContent).join('|')`)).includes("Sales board"));
  await widgetsOfEachView(page);
  await refreshing(page, dir);
  await failures(page, dir);
  await arranging(page);
  await saving(page, dir);
  await files(page, dir);
  await autoRefresh(page, dir);
}
