// The README's screenshots on a throwaway SQLite shop: the data viewer sorted from its header
// with the value panel, JOIN completion, the data editor's pending changes, the table editor,
// the visual query builder, the connection dialog, the command palette, Settings and the light
// theme. Runs only with PLATYPUS_README_SHOTS set to the folder the pictures go to
// (docs/screenshots); the filter below also runs readme-duckdb and readme-postgres, which use
// the helpers exported here:
//
//   PLATYPUS_README_SHOTS=../../docs/screenshots npm run e2e -- readme
import { writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

export const NAME = "Shop dev";
export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
export const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
export const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (title) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(title)}))`;

export function skip() {
  return process.env.PLATYPUS_README_SHOTS ? null : "PLATYPUS_README_SHOTS not set";
}

const CUSTOMERS = [
  ["Giulia Ferrari", "Milano"], ["Marco Conti", "Torino"], ["Sara Greco", "Udine"],
  ["Luca Bianchi", "Bologna"], ["Elena Russo", "Napoli"], ["Paolo Romano", "Roma"],
  ["Chiara Gallo", "Trieste"], ["Davide Costa", "Genova"], ["Anna Fontana", null],
  ["Matteo Rizzo", "Verona"], ["Francesca Moretti", "Padova"], ["Simone Marino", "Bari"],
];
const PRODUCTS = [
  ["Espresso machine", 389], ["Grinder", 129.9], ["Milk jug", 18.5], ["Coffee beans 1 kg", 24],
  ["Descaler", 12.9], ["Tamper", 34], ["Cups (6)", 42], ["Scale", 59],
];
const STATUSES = ["paid", "paid", "shipped", "paid", "delivered", "pending", "shipped", "refunded"];

export function setup({ dir }) {
  const path = join(dir, "shop.db");
  const db = new DatabaseSync(path);
  db.exec(`
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT, email TEXT UNIQUE);
    CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price NUMERIC NOT NULL);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers (id),
      placed_at TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', total NUMERIC, details TEXT);
    CREATE TABLE order_items (order_id INTEGER REFERENCES orders (id), product_id INTEGER REFERENCES products (id),
      qty INTEGER NOT NULL, PRIMARY KEY (order_id, product_id));
    CREATE INDEX orders_customer ON orders (customer_id);
    CREATE VIEW paid_orders AS SELECT * FROM orders WHERE status = 'paid';
  `);
  const customer = db.prepare("INSERT INTO customers VALUES (?, ?, ?, ?)");
  CUSTOMERS.forEach(([name, city], i) =>
    customer.run(i + 1, name, city, `${name.split(" ")[0].toLowerCase()}@example.com`),
  );
  const product = db.prepare("INSERT INTO products VALUES (?, ?, ?)");
  PRODUCTS.forEach(([name, price], i) => product.run(i + 1, name, price));
  const order = db.prepare("INSERT INTO orders VALUES (?, ?, ?, ?, ?, ?)");
  const item = db.prepare("INSERT INTO order_items VALUES (?, ?, ?)");
  for (let i = 0; i < 40; i++) {
    const id = 2941 + i;
    const lines = [0, 1, 2].slice(0, 1 + (i % 3)).map((k) => ({ product: 1 + ((i * 3 + k * 5) % PRODUCTS.length), qty: 1 + ((i + k) % 3) }));
    const total = lines.reduce((sum, l) => sum + PRODUCTS[l.product - 1][1] * l.qty, 0);
    const day = String(1 + Math.floor(i / 2)).padStart(2, "0");
    const time = `${String(8 + ((i * 7) % 13)).padStart(2, "0")}:${String((i * 17) % 60).padStart(2, "0")}`;
    const details = { channel: i % 4 === 0 ? "store" : "web", items: lines, gift: i % 5 === 0 };
    order.run(id, 1 + ((i * 5) % CUSTOMERS.length), `2026-10-${day} ${time}`, STATUSES[i % STATUSES.length],
      Math.round(total * 100) / 100, JSON.stringify(details));
    for (const l of lines) item.run(id, l.product, l.qty);
  }
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "00000000-0000-4000-9000-000000000031"
name = "Shop"
color = "#d97706"
connections = ["00000000-0000-4000-8000-000000000031"]

[[projects]]
id = "00000000-0000-4000-9000-000000000032"
name = "Analytics"
color = "#0d9488"

[[connections]]
id = "00000000-0000-4000-8000-000000000031"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
connection_type = "dev"
`,
  );
  writeFileSync(join(dir, "settings.toml"), '[appearance]\ntheme = "platypus_dark"\n');
}

/** Right-clicks the navigator row `label` and runs its entry starting with `title`. */
export async function navigatorMenu(page, label, title) {
  await page.eval(`${treeRow(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`${menuButton(title)} !== undefined`);
  await page.eval(`${menuButton(title)}.click()`);
}

/** Runs `action` and waits for the end of the run it starts: Cancel disabled again and a new
 *  status, emptied first as the old one may read the same. */
async function afterRun(page, action) {
  await page.eval(`(() => { const s = ${visibleTab}?.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing')`, 15_000);
}

export async function openTable(page, name) {
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith(${JSON.stringify(name)})`, 15_000);
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar') != null && /^\\d+ rows$/.test(${status})`, 15_000);
}

/** Replaces the focused cell's value with `text`; Enter saves it and moves down. */
async function editCell(page, text) {
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
  await page.key("ArrowUp", "ArrowUp", 38);
}

export async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
}

/** Lays the page out at 1600×960, where the toolbar fits on one line. */
async function wide(page) {
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1600, height: 960, deviceScaleFactor: 1, mobile: false });
  await page.sleep(300);
}

/** Numbers and times in British English (a 24-hour clock that fits the query manager): the page
 *  formats them in the locale of Windows, which would change the pictures from one machine to
 *  the next. Runs before the app's scripts. */
const ENGLISH_FORMATS = `(() => {
  for (const name of ["NumberFormat", "DateTimeFormat", "RelativeTimeFormat", "PluralRules", "ListFormat"]) {
    Intl[name] = new Proxy(Intl[name], {
      construct: (target, [locales, ...rest]) => new target(locales ?? "en-GB", ...rest),
      apply: (target, self, [locales, ...rest]) => target(locales ?? "en-GB", ...rest),
    });
  }
  for (const [proto, names] of [[Number.prototype, ["toLocaleString"]], [Date.prototype, ["toLocaleString", "toLocaleDateString", "toLocaleTimeString"]]]) {
    for (const name of names) {
      const format = proto[name];
      proto[name] = function (locales, options) { return format.call(this, locales ?? "en-GB", options); };
    }
  }
})()`;

/** Text cursors that do not blink, so a picture never catches one half drawn. */
const STEADY_CURSORS = `(() => {
  const style = document.createElement("style");
  style.textContent = "input, textarea { caret-color: transparent !important } .cm-cursorLayer { animation: none !important }";
  document.head.append(style);
})()`;

/** The window as every README picture shows it: 1600×960, English formats, steady cursors and
 *  the bundled fonts loaded. Loads the page again, so it comes first. */
export async function prepare(page) {
  // WebView2 runs the added script only with the Page domain enabled.
  await page.send("Page.enable");
  await page.send("Page.addScriptToEvaluateOnNewDocument", { source: ENGLISH_FORMATS });
  // The marker only exists on the old document: its absence means the new one has loaded.
  await page.eval(`window.__e2eBeforeReload = true; location.reload()`);
  await page.waitFor(`window.__e2eBeforeReload === undefined && document.querySelector('.tree-row') !== null`, 30_000);
  await page.eval(STEADY_CURSORS);
  await wide(page);
  await page.waitFor(`document.fonts.status === 'loaded'`);
}

/** Shows the project `name`, the `n`th of the rail (Ctrl+n). */
export async function openProject(page, n, name) {
  await page.waitFor(`document.querySelectorAll('.project-square').length > 1`);
  await page.key(String(n), `Digit${n}`, 48 + n, CTRL);
  await page.waitFor(`document.querySelector('.navigator-project')?.textContent === ${JSON.stringify(name)}`);
}

export async function closeTab(page) {
  await page.key("w", "KeyW", 87, CTRL);
  await page.sleep(300);
}

export default async function (page) {
  const out = resolve(process.env.PLATYPUS_README_SHOTS);
  const shot = (name) => page.screenshot(join(out, `${name}.png`));
  await prepare(page);
  await openProject(page, 2, "Shop");
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("orders")} !== undefined`, 15_000);

  // Data viewer: orders, newest first from two clicks on the id header (which write the
  // ORDER BY in the editor), the customer names from the foreign key and the JSON details of
  // the focused row in the value panel.
  await openTable(page, "orders");
  const header = await page.eval(`(() => { const r = ${grid}.getBoundingClientRect(); return { x: r.x + 50, y: r.y + 14 }; })()`);
  await afterRun(page, () => page.click(header.x, header.y));
  await afterRun(page, () => page.click(header.x, header.y));
  await page.waitFor(`${editorText}.endsWith('ORDER BY id DESC')`);
  await page.eval(`${grid}.focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < 5; i++) await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("F7", "F7", 118);
  await page.waitFor(`document.querySelector('.value-text')?.textContent.includes('"items"')`);
  await page.sleep(300);
  await shot("data-viewer");
  await page.key("F7", "F7", 118);
  await page.waitFor(`document.querySelector('.value-panel') === null`);

  // Data editor: a changed cell, a deleted row and a new one, highlighted until saved.
  await openTable(page, "customers");
  await page.eval(`${grid}.focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("ArrowRight", "ArrowRight", 39);
  await editCell(page, "Asti");
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("Delete", "Delete", 46);
  await page.key("Insert", "Insert", 45);
  await page.key("ArrowLeft", "ArrowLeft", 37);
  await editCell(page, "Irene Villa");
  await page.key("ArrowRight", "ArrowRight", 39);
  await editCell(page, "Pisa");
  await page.waitFor(`${visibleTab}.querySelector('.edit-bar').textContent.includes('3 rows changed')`);
  await page.sleep(300);
  await shot("data-editor");
  await closeTab(page);

  // Table editor on orders: columns, keys and the foreign key to customers.
  await navigatorMenu(page, "orders", "Edit table");
  const tableEditor = `[...document.querySelectorAll('.table-editor')].find(t => t.style.display !== 'none')`;
  await page.waitFor(`${tableEditor}?.querySelectorAll('tbody tr').length >= 6`, 10_000);
  await page.sleep(300);
  await shot("table-editor");
  await closeTab(page);

  // Visual query builder: customers joined to orders on their foreign key.
  await navigatorMenu(page, "customers", "Visual query builder");
  const builderSql = `document.querySelector('.builder-tab:not([style*="none"]) .builder-sql')?.textContent ?? ''`;
  await page.waitFor(`${builderSql}.includes('FROM customers c')`, 10_000);
  await page.waitFor(`[...document.querySelectorAll('.builder-source')].some(r => r.textContent === 'orders')`, 10_000);
  await page.eval(`[...document.querySelectorAll('.builder-source')].find(r => r.textContent === 'orders').id = 'e2e-orders'`);
  await page.clickSel("#e2e-orders", { clickCount: 2 });
  await page.waitFor(`${builderSql}.includes('JOIN orders o ON o.customer_id = c.id')`, 10_000);
  const tick = (table, column) =>
    `[...document.querySelectorAll('.builder-box[data-table="${table}"] label')].find(l => l.textContent === '${column}').querySelector('input').click()`;
  for (const [table, column] of [["customers", "name"], ["customers", "city"], ["orders", "placed_at"], ["orders", "total"]]) {
    await page.eval(tick(table, column));
  }
  await page.waitFor(`${builderSql}.includes('o.total')`);
  await page.sleep(300);
  await shot("query-builder");
  await closeTab(page);

  // SQL editor: JOIN completes from the foreign keys.
  await page.eval(`${treeRow(NAME)}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-connection')?.removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 10_000);
  await setEditor(page, "-- Revenue by city\nSELECT c.city, count(*) AS orders, sum(o.total) AS revenue\nFROM orders o\nJOIN ");
  await page.key(" ", "Space", 32, CTRL);
  await page.waitFor(`[...document.querySelectorAll('.cm-tooltip-autocomplete li')].some(li => li.textContent.includes('customers c ON'))`, 10_000);
  await page.sleep(300);
  await shot("autocomplete");
  await page.key("Escape", "Escape", 27);

  // Results of that query, in Platypus Light.
  await setEditor(
    page,
    "-- Revenue by city\nSELECT c.city, count(*) AS orders, sum(o.total) AS revenue\nFROM orders o\n" +
      "JOIN customers c ON o.customer_id = c.id\nWHERE o.status <> 'refunded'\nGROUP BY c.city\nORDER BY revenue DESC",
  );
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`/^\\d+ rows$/.test(${status})`, 15_000);
  await page.eval(`document.activeElement?.blur()`);
  const toggle = `document.querySelector('button[title^="Theme:"]')`;
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${toggle}.title.startsWith('Theme: System')`);
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`getComputedStyle(document.documentElement).getPropertyValue('--bg').trim() === '#ffffff'`);
  await page.sleep(300);
  await shot("light-theme");
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`getComputedStyle(document.documentElement).getPropertyValue('--bg').trim() === '#16191b'`);

  // Data analysis: the same results as bars of the revenue by city.
  const viewToggle = (label) => `[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === '${label}')`;
  await page.eval(`${viewToggle("Chart")}.click()`);
  await page.waitFor(`${visibleTab}.querySelectorAll('.chart-svg rect.chart-bar').length > 0`, 10_000);
  await page.eval(
    `[...${visibleTab}.querySelectorAll('.chart-side fieldset label')].find(l => l.textContent === 'orders').querySelector('input').click()`,
  );
  await page.waitFor(`${visibleTab}.querySelector('.chart-svg .chart-legend text') === null`);
  await page.sleep(300);
  await shot("chart");
  await page.eval(`${viewToggle("Grid")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.chart-svg') === null`);

  // Command palette.
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type("orders");
  await page.waitFor(`document.querySelector('.palette li.selected') !== null`);
  await page.sleep(300);
  await shot("palette");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);

  // Settings, one section per tab: the themes.
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Appearance");
  await page.eval(`document.activeElement?.blur()`);
  await page.sleep(300);
  await shot("settings");
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  // New connection: the grid of databases.
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.querySelector('.driver-grid') !== null`);
  await page.sleep(300);
  await shot("connections");
  await page.key("Escape", "Escape", 27);
}
