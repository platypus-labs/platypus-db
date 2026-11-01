// The visual inventory: every screen and dialog of the app, photographed once per variant
// (dark/light theme x English/Italian), for a reviewer to go through and note what looks wrong.
// The four scenarios `inventory-<theme>-<language>.mjs` seed a profile with `seed` and walk the
// list below with `runInventory`. They run only with PLATYPUS_INVENTORY set, and the pictures
// go to <PLATYPUS_E2E_SHOTS>/inventory/<variant>/<NN-name>.png. See docs/INVENTORY.md.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "./cdp.mjs";
import { connect as netConnect } from "node:net";
import { openSubmenu, revealItem } from "./menu-sub.mjs";
import { setField } from "./plans.mjs";

const here = dirname(fileURLToPath(import.meta.url));

export const NAME = "Shop dev";
const ARCHIVE = "Archive db";
const PROJECT = "00000000-0000-4000-9000-0000000a0031";
const ANALYTICS = "00000000-0000-4000-9000-0000000a0032";
// The dev PostgreSQL of docker/compose.yml, for the screens that need a server (kept out of the
// Shop project, so the shop screens stay as they were); the screens skip when it is not up.
const PG = "Local Postgres";
const PG_ID = "00000000-0000-4000-8000-0000000a0003";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The scenarios' `skip`: the inventory only runs when asked for. */
export const skip = () => (process.env.PLATYPUS_INVENTORY ? null : "PLATYPUS_INVENTORY not set");

// The Italian texts of the interface, so the scenarios find a button by its English name.
let italian = null;
let language = "en";
function tr(english) {
  if (language !== "it") return english;
  italian ??= readFileSync(resolve(here, "../src/i18n/it.ts"), "utf8");
  const key = JSON.stringify(english).slice(1, -1).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const found = italian.match(new RegExp(`^\\s*"${key}":\\s*("(?:[^"\\\\]|\\\\.)*"),?\\s*$`, "m"));
  return found ? JSON.parse(found[1]) : english;
}

const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const SQLITE_PLAN = "QUERY PLAN\n|--SCAN o\n`--SEARCH c USING INTEGER PRIMARY KEY (rowid=?)";

/** Seeds the profile of a variant: the shop database, an empty one to copy to, two projects
 *  and the theme and language in settings.toml. */
export function seed(dir, { theme, lang }) {
  language = lang;
  const path = join(dir, "shop.db");
  const db = new DatabaseSync(path);
  db.exec(`
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT, email TEXT UNIQUE);
    CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price NUMERIC NOT NULL);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers (id),
      placed_at TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', total NUMERIC);
    CREATE TABLE order_items (order_id INTEGER REFERENCES orders (id), product_id INTEGER REFERENCES products (id),
      qty INTEGER NOT NULL, PRIMARY KEY (order_id, product_id));
    CREATE TABLE empty_notes (id INTEGER PRIMARY KEY, body TEXT);
    CREATE INDEX orders_customer ON orders (customer_id);
    CREATE VIEW paid_orders AS SELECT * FROM orders WHERE status = 'paid';
  `);
  const cities = ["Milano", "Torino", "Udine", "Bologna", "Napoli", "Roma", "Trieste", "Genova"];
  const names = ["Giulia Ferrari", "Marco Conti", "Sara Greco", "Luca Bianchi", "Elena Russo", "Paolo Romano", "Chiara Gallo", "Davide Costa"];
  names.forEach((n, i) => db.prepare("INSERT INTO customers VALUES (?, ?, ?, ?)").run(i + 1, n, i === 5 ? null : cities[i], `${n.split(" ")[0].toLowerCase()}@example.com`));
  [["Espresso machine", 389], ["Grinder", 129.9], ["Milk jug", 18.5], ["Coffee beans 1 kg", 24]].forEach(([n, p], i) => db.prepare("INSERT INTO products VALUES (?, ?, ?)").run(i + 1, n, p));
  const statuses = ["paid", "shipped", "paid", "delivered", "pending", "refunded"];
  for (let i = 0; i < 24; i++) {
    db.prepare("INSERT INTO orders VALUES (?, ?, ?, ?, ?)").run(2941 + i, 1 + (i % 8), `2026-10-${String(1 + i).padStart(2, "0")} 10:${String(i * 2).padStart(2, "0")}`, statuses[i % 6], 20 + i * 13.5);
    db.prepare("INSERT INTO order_items VALUES (?, ?, ?)").run(2941 + i, 1 + (i % 4), 1 + (i % 3));
  }
  db.close();
  new DatabaseSync(join(dir, "archive.db")).close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "${PROJECT}"
name = "Shop"
color = "#d97706"
connections = ["00000000-0000-4000-8000-0000000a0001"]

[[projects]]
id = "00000000-0000-4000-9000-0000000a0032"
name = "Analytics"
color = "#0d9488"
connections = ["${PG_ID}"]

[[connections]]
id = "00000000-0000-4000-8000-0000000a0001"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
connection_type = "dev"

[[connections]]
id = "00000000-0000-4000-8000-0000000a0002"
name = "${ARCHIVE}"
driver = "sqlite"
database = '${join(dir, "archive.db")}'
save_password = false

[[connections]]
id = "${PG_ID}"
name = "${PG}"
driver = "postgres"
host = "127.0.0.1"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
password_command = "echo platypus"
`,
  );
  writeFileSync(join(dir, "settings.toml"), `[appearance]\ntheme = "${theme}"\nlanguage = "${lang}"\n`);
}

// Helpers that drive the page by what the user sees (texts in the language of the variant).

const key = (page, name, code, vk, mods = 0) => page.key(name, code, vk, mods);
const escape = (page) => key(page, "Escape", "Escape", 27);

async function palette(page, title) {
  await key(page, "k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type(tr(title));
  await page.waitFor(`document.querySelector('.palette li.selected')?.textContent.includes(${JSON.stringify(tr(title))})`);
  await key(page, "Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.palette') === null`);
}

async function rightClick(page, rowLabel) {
  await page.waitFor(`${treeRow(rowLabel)} !== undefined`, 15_000);
  await page.eval(`${treeRow(rowLabel)}.querySelector('.tree-label').id = 'inv-row'`);
  await page.clickSel("#inv-row", { button: "right" });
  await page.eval(`document.getElementById('inv-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

/** Right-clicks the navigator row `rowLabel` and clicks the menu entry starting with `item`. */
async function menuItem(page, rowLabel, item) {
  await rightClick(page, rowLabel);
  await revealItem(page, tr(item));
  const entry = `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(tr(item))}))`;
  await page.waitFor(`${entry} !== undefined`);
  await page.eval(`${entry}.click()`);
}

/** Expands the shop connection down to its tables. */
async function expandShop(page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`, 15_000);
  const orders = `${treeRow("orders")} !== undefined`;
  // Each pass opens the first closed row below the connection: its schema, then its tables.
  for (let i = 0; i < 4 && !(await page.eval(orders)); i++) {
    await page.eval(`(() => {
      const all = [...document.querySelectorAll('.tree-row')];
      const at = all.findIndex(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)});
      const level = Number(all[at].getAttribute('aria-level'));
      for (let j = at; j < all.length && (j === at || Number(all[j].getAttribute('aria-level')) > level); j++) {
        if (all[j].getAttribute('aria-expanded') === 'false') { all[j].querySelector('.twisty')?.click(); return; }
      }
    })()`);
    await page.sleep(500);
  }
  await page.waitFor(orders, 15_000);
}

async function openTable(page, name) {
  await expandShop(page);
  await page.eval(`${treeRow(name)}.querySelector('.tree-label').id = 'inv-table'`);
  await page.clickSel("#inv-table", { clickCount: 2 });
  await page.eval(`document.getElementById('inv-table')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith(${JSON.stringify(name)})`, 15_000);
  await page.waitFor(`${queryTab}?.querySelector('.edit-bar') != null && /\\d/.test(${queryTab}.querySelector('.statusbar span')?.textContent ?? '')`, 15_000);
}

async function setEditor(page, sql) {
  await page.eval(`${queryTab}.querySelector('.cm-content').focus()`);
  await key(page, "a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${queryTab}.querySelectorAll('.editor .cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
}

/** Runs the editor's text and waits for the end of the run. */
async function run(page) {
  await page.eval(`(() => { const s = ${queryTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await key(page, "Enter", "Enter", 13, CTRL);
  await page.waitFor(`(() => { const s = ${queryTab}.querySelector('.statusbar span')?.textContent ?? ''; return s !== '' && !s.includes('…'); })()`, 15_000);
}

async function newEditor(page) {
  await expandShop(page);
  await page.eval(`${treeRow(NAME)}.querySelector('.tree-label').id = 'inv-conn'`);
  await page.clickSel("#inv-conn", { clickCount: 2 });
  await page.eval(`document.getElementById('inv-conn')?.removeAttribute('id')`);
  await page.waitFor(`${queryTab}?.querySelector('.cm-content') != null`, 15_000);
}

/** Closes dialogs and every tab, discarding what asks to be kept, back to the welcome page. */
async function reset(page) {
  for (let i = 0; i < 6; i++) {
    if (await page.eval(`document.querySelector('.modal, .palette, .context-menu') === null`)) break;
    await escape(page);
    await sleep(150);
  }
  for (let i = 0; i < 20; i++) {
    if (await page.eval(`document.querySelectorAll('.tabs .tab').length === 0`)) break;
    await key(page, "w", "KeyW", 87, CTRL);
    await sleep(250);
    // A question about unsaved changes: the button that does not keep them.
    await page.eval(`(() => {
      const m = document.querySelector('.modal.confirm');
      if (!m) return;
      const b = [...m.querySelectorAll('button')];
      (b.find(x => x.classList.contains('danger')) ?? b.find(x => /discard|don't save|close|chiudi|scarta|non salvare/i.test(x.textContent)) ?? b.at(-1)).click();
    })()`);
    await sleep(150);
  }
}

const clickText = (page, scope, text) =>
  page.eval(`[...document.querySelectorAll(${JSON.stringify(scope)})].find(b => b.textContent.trim().startsWith(${JSON.stringify(tr(text))})).click()`);

// The screens: `name` for the file and the doc, `run(page, shoot)` takes the pictures. A screen
// that cannot be reached is noted and left out; it never stops the others.

export const screens = [
  { name: "welcome", run: async (page, shoot) => shoot() },
  {
    name: "navigator",
    run: async (page, shoot) => {
      await expandShop(page);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "sql-editor-result",
    run: async (page, shoot) => {
      await newEditor(page);
      await setEditor(page, "SELECT c.city, count(*) AS orders, sum(o.total) AS revenue\nFROM orders o\nJOIN customers c ON o.customer_id = c.id\nGROUP BY c.city\nORDER BY revenue DESC");
      await run(page);
      await shoot();
    },
  },
  {
    name: "query-history",
    run: async (page, shoot) => {
      await newEditor(page);
      await setEditor(page, "SELECT * FROM customers");
      await run(page);
      await page.eval(`[...${queryTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(tr("History"))}).click()`);
      await page.waitFor(`document.querySelector('.history') !== null`);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "error-state",
    run: async (page, shoot) => {
      await newEditor(page);
      await setEditor(page, "SELECT * FROM missing_table");
      await run(page);
      await shoot();
    },
  },
  {
    name: "empty-state",
    run: async (page, shoot) => {
      await newEditor(page);
      await setEditor(page, "SELECT * FROM orders WHERE 1 = 0");
      await run(page);
      await shoot();
    },
  },
  {
    name: "grid-unsaved-edit",
    run: async (page, shoot) => {
      await openTable(page, "customers");
      await page.eval(`${queryTab}.querySelector('.grid-scroll').focus()`);
      await key(page, "ArrowDown", "ArrowDown", 40);
      await key(page, "ArrowRight", "ArrowRight", 39);
      await key(page, "ArrowRight", "ArrowRight", 39);
      await key(page, "F2", "F2", 113);
      await page.waitFor(`document.querySelector('.cell-editor') !== null`);
      await key(page, "a", "KeyA", 65, CTRL);
      await page.type("Asti");
      await key(page, "Enter", "Enter", 13);
      await page.waitFor(`document.querySelector('.cell-editor') === null`);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "object-page",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "orders", "Open details");
      await page.waitFor(`document.querySelector('.object-tab .object-sections') !== null`, 15_000);
      const count = await page.eval(`document.querySelectorAll('.object-tab .object-sections button').length`);
      for (let i = 0; i < count; i++) {
        await page.eval(`document.querySelectorAll('.object-tab .object-sections button')[${i}].click()`);
        await page.sleep(500);
        const label = await page.eval(`document.querySelectorAll('.object-tab .object-sections button')[${i}].firstChild.textContent`);
        await shoot(label);
      }
    },
  },
  {
    name: "table-editor",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "orders", "Edit table…");
      await page.waitFor(`document.querySelectorAll('.table-editor tbody tr').length >= 4`, 15_000);
      await page.sleep(400);
      await shoot();
    },
  },
  {
    name: "er-diagram",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "main", "ER diagram");
      await page.waitFor(`document.querySelector('.erd-stage') !== null`, 15_000);
      await page.sleep(800);
      await shoot();
    },
  },
  {
    name: "explain-plan",
    run: async (page, shoot) => {
      await palette(page, "Open query plan…");
      await page.waitFor(`document.querySelector('.plan-dialog textarea') !== null`);
      await shoot("dialog");
      await setField(page, ".plan-dialog textarea", SQLITE_PLAN);
      await page.waitFor(`!document.querySelector('.plan-dialog button[type=submit]').disabled`);
      await page.eval(`document.querySelector('.plan-dialog button[type=submit]').click()`);
      await page.waitFor(`document.querySelector('.explain-tab .explain-op') !== null`, 10_000);
      await page.sleep(300);
      await shoot("plan");
    },
  },
  {
    name: "table-profile",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "orders", "Profile data…");
      await page.waitFor(`document.querySelectorAll('.tp-card').length > 2`, 15_000);
      await page.sleep(500);
      await shoot();
    },
  },
  {
    name: "dashboard",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, NAME, "New dashboard");
      await page.waitFor(`document.querySelector('.dashboard-tab') !== null`, 10_000);
      await page.sleep(400);
      await shoot();
    },
  },
  {
    name: "export-dialog",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "customers", "Export data…");
      await page.waitFor(`document.querySelector('.modal.export-tables') !== null`);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "import-dialog",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "customers", "Import data…");
      await page.waitFor(`document.querySelector('.modal.import-data') !== null`);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "transfer",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "main", "Copy schema to a database…");
      await page.waitFor(`document.querySelector('.modal.migrate select')?.selectedOptions[0] != null`, 15_000);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "driver-picker",
    run: async (page, shoot) => {
      await page.eval(`document.querySelector('.sidebar-column button[aria-label=${JSON.stringify(tr("New connection"))}]').click()`);
      await page.waitFor(`document.querySelector('.driver-grid') !== null`);
      await page.sleep(300);
      await shoot();
    },
  },
  {
    name: "connection-form",
    run: async (page, shoot) => {
      await page.eval(`document.querySelector('.sidebar-column button[aria-label=${JSON.stringify(tr("New connection"))}]').click()`);
      await page.waitFor(`document.querySelector('.driver-grid') !== null`);
      await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent.includes('PostgreSQL')).click()`);
      await page.waitFor(`document.querySelector('.dialog-tabs [role=tab]') !== null`);
      const count = await page.eval(`document.querySelectorAll('.modal .dialog-tabs [role=tab]').length`);
      for (let i = 0; i < count; i++) {
        await page.eval(`document.querySelectorAll('.modal .dialog-tabs [role=tab]')[${i}].click()`);
        await page.sleep(300);
        await shoot(String(i + 1));
      }
    },
  },
  {
    name: "projects",
    run: async (page, shoot) => {
      await page.waitFor(`document.querySelectorAll('.project-square').length > 1`);
      await shoot("rail");
      await page.eval(`document.querySelector('.project-add').click()`);
      await page.waitFor(`document.querySelector('.project-dialog') !== null`);
      await page.sleep(300);
      await shoot("dialog");
    },
  },
  {
    name: "command-palette",
    run: async (page, shoot) => {
      await key(page, "k", "KeyK", 75, CTRL);
      await page.waitFor(`document.querySelector('.palette input') !== null`);
      await page.type("or");
      await page.waitFor(`document.querySelector('.palette li.selected') !== null`);
      await page.sleep(300);
      await shoot();
    },
  },
  { name: "settings", run: settingsScreens },
  {
    name: "test-connections",
    run: async (page, shoot) => {
      const r = await page.rect(".sidebar .tree");
      await page.click(r.x + 40, r.y + r.h - 20, { button: "right" });
      await page.waitFor(`document.querySelector('.context-menu') !== null`);
      await clickText(page, ".context-menu button", "Test connections");
      await page.waitFor(`(document.querySelector('.test-connections [data-field=summary]')?.textContent ?? '') !== ''`, 30_000);
      await shoot();
    },
  },
  {
    name: "confirm-dialog",
    run: async (page, shoot) => {
      await expandShop(page);
      await menuItem(page, "empty_notes", "Drop…");
      await page.waitFor(`document.querySelector('.modal') !== null`);
      await page.sleep(300);
      await shoot();
    },
  },
  ...moreScreens(),
];

// ---- the screens of the UX review (group T): one per section of its checklist ----

const reachable = (port) =>
  new Promise((done) => {
    const socket = netConnect({ host: "127.0.0.1", port, timeout: 1500 });
    socket.once("connect", () => (socket.destroy(), done(true)));
    socket.once("error", () => done(false));
    socket.once("timeout", () => (socket.destroy(), done(false)));
  });

/** Sets a React-controlled input or select the way typing or choosing would. */
const setValue = (page, element, value, proto = "HTMLInputElement") =>
  page.eval(`(() => {
    const el = ${element};
    Object.getOwnPropertyDescriptor(${proto}.prototype, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(${proto === "HTMLSelectElement" ? "'change'" : "'input'"}, { bubbles: true }));
  })()`);

/** Runs `fn` in the Analytics project, where the dev PostgreSQL lives, and goes back to Shop. */
async function inPostgres(page, fn) {
  if (!(await reachable(55432))) throw new Error("PostgreSQL at localhost:55432 is not reachable (docker compose -f docker/compose.yml up -d --wait)");
  await page.eval(`document.querySelector('.project-square[data-project="${ANALYTICS}"]').click()`);
  try {
    await page.waitFor(`${treeRow(PG)} !== undefined`, 15_000);
    await page.eval(`${treeRow(PG)}.querySelector('.twisty')?.click()`);
    await page.waitFor(`(() => {
      const all = [...document.querySelectorAll('.tree-row')];
      const at = all.findIndex(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(PG)});
      return at >= 0 && all[at + 1] !== undefined && Number(all[at + 1].getAttribute('aria-level')) > Number(all[at].getAttribute('aria-level'));
    })()`, 30_000);
    await fn();
  } finally {
    await page.eval(`document.querySelector('.project-square[data-project="${PROJECT}"]')?.click()`);
  }
}

/** Shoots every button of an `.object-sections` bar of `scope`, one picture per section. */
async function eachSection(page, shoot, scope, wait = 800) {
  const buttons = `document.querySelectorAll(${JSON.stringify(`${scope} .object-sections button`)})`;
  await page.waitFor(`${buttons}.length > 0`, 20_000);
  const labels = await page.eval(`[...${buttons}].map(b => b.firstChild?.textContent ?? b.textContent)`);
  for (let i = 0; i < labels.length; i++) {
    await page.eval(`${buttons}[${i}].click()`);
    await page.sleep(wait);
    await shoot(labels[i]);
  }
}

const adminTab = `[...document.querySelectorAll('.admin-tab')].find(t => t.style.display !== 'none')`;
const notebookTab = `[...document.querySelectorAll('.notebook-tab')].find(t => t.style.display !== 'none')`;
const builderTab = `document.querySelector('.builder-tab:not([style*="none"])')`;
const cellOf = (name) => `${notebookTab}?.querySelector('[data-name=${JSON.stringify(name)}]')`;

async function notebookScreens(page, shoot) {
  await expandShop(page);
  await menuItem(page, NAME, "New notebook");
  await page.waitFor(`${cellOf("c1")}?.querySelector('.cm-content') != null`, 15_000);
  await page.sleep(300);
  await shoot("empty");
  const fill = async (name, sql) => {
    await page.eval(`${cellOf(name)}.querySelector('.cm-content').focus()`);
    await key(page, "a", "KeyA", 65, CTRL);
    await page.type(sql);
    await page.waitFor(`[...${cellOf(name)}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
    await escape(page);
    await page.eval(`${cellOf(name)}.querySelector('[data-action="run"]').click()`);
    await page.waitFor(`!${cellOf(name)}.classList.contains('running') && (${cellOf(name)}.querySelector('[data-field=status]')?.dataset.result ?? '') !== ''`, 30_000);
  };
  await fill("c1", "SELECT status, count(*) AS orders, sum(total) AS revenue FROM orders GROUP BY status");
  await page.eval(`${notebookTab}.querySelector('[data-action="add_sql"]').click()`);
  await page.waitFor(`${cellOf("c2")}?.querySelector('.cm-content') != null`);
  await fill("c2", "SELECT name, price FROM products ORDER BY price DESC");
  await page.sleep(300);
  await shoot("cells");
}

async function chartPivotScreens(page, shoot) {
  await newEditor(page);
  await setEditor(page, "SELECT c.city, count(*) AS orders, sum(o.total) AS revenue\nFROM orders o\nJOIN customers c ON o.customer_id = c.id\nGROUP BY c.city\nORDER BY c.city");
  await run(page);
  await page.eval(`[...${queryTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === ${JSON.stringify(tr("Chart"))}).click()`);
  await page.waitFor(`${queryTab}.querySelector('.chart-svg') !== null`, 10_000);
  await page.sleep(500);
  await shoot("bar");
  await setValue(page, `${queryTab}.querySelector('select[aria-label=${JSON.stringify(tr("Chart type"))}]')`, "pie", "HTMLSelectElement");
  await page.waitFor(`${queryTab}.querySelector('path.chart-slice') !== null`);
  await page.sleep(300);
  await shoot("pie");
  await page.eval(`[...${queryTab}.querySelectorAll('.view-toggle button')][0].click()`);
  await page.waitFor(`${queryTab}.querySelector('.grid-scroll') !== null`);
  await page.eval(`${queryTab}.querySelector('.result-pivot-open').click()`);
  await page.waitFor(`document.querySelector('.pivot-dialog') !== null`);
  await page.sleep(300);
  await shoot("dialog");
  await page.eval(`document.querySelector('.pivot-dialog fieldset label input').click()`);
  await page.eval(`[...document.querySelectorAll('.pivot-dialog .actions button')].find(b => b.textContent === ${JSON.stringify(tr("Pivot"))}).click()`);
  await page.waitFor(`document.querySelector('.pivot-dialog') === null`, 15_000);
  await page.sleep(600);
  await shoot("result");
}

async function builderScreens(page, shoot) {
  await expandShop(page);
  await menuItem(page, "customers", "Visual query builder");
  await page.waitFor(`(${builderTab}?.querySelector('.builder-sql')?.textContent ?? '').includes('FROM customers')`, 15_000);
  await page.sleep(500);
  await shoot("one-table");
  await page.eval(`[...${builderTab}.querySelectorAll('.builder-source')].find(r => r.textContent === 'orders').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${builderTab}.querySelector('.builder-box[data-table="orders"] label') !== null`, 10_000);
  await page.eval(`${builderTab}.querySelector('.builder-box[data-table="customers"] label input').click()`);
  await page.eval(`${builderTab}.querySelector('.builder-box[data-table="orders"] label input').click()`);
  await page.sleep(500);
  await shoot("joined");
}

async function adminScreens(page, shoot) {
  await inPostgres(page, async () => {
    await menuItem(page, PG, "Server activity");
    await page.waitFor(`${adminTab} !== undefined`, 30_000);
    await eachSection(page, shoot, ".admin-tab", 1500);
  });
}

async function schemaToolsScreens(page, shoot) {
  await expandShop(page);
  await menuItem(page, "main", "Compare with…");
  const dialog = `document.querySelector('.modal.schema-compare')`;
  await page.waitFor(`${dialog} !== null`);
  await setValue(page, `${dialog}.querySelectorAll('select')[0]`, await page.eval(`[...${dialog}.querySelectorAll('select')[0].options].find(o => o.textContent === ${JSON.stringify(ARCHIVE)}).value`), "HTMLSelectElement");
  await page.waitFor(`[...${dialog}.querySelectorAll('select')[1].options].some(o => o.textContent === 'main')`, 15_000);
  await setValue(page, `${dialog}.querySelectorAll('select')[1]`, "main", "HTMLSelectElement");
  await page.sleep(300);
  await shoot("compare-dialog");
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(tr("Compare"))}).click()`);
  await page.waitFor(`${dialog} === null || ${dialog}.querySelector('.notice') !== null`, 30_000);
  await page.sleep(800);
  await shoot("compare-result");
  await reset(page);
  await expandShop(page);
  await menuItem(page, "main", "Check schema…");
  await page.waitFor(`document.querySelector('.modal.schema-lint') !== null`, 15_000);
  await page.sleep(1500);
  await shoot("check");
}

async function lockScreens(page, shoot) {
  const invoke = (command) => page.eval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, { password: 'inventory-pw' }).then(() => true)`);
  await invoke("enable_vault");
  try {
    await palette(page, "Lock PlatypusDB");
    await page.waitFor(`document.querySelector('.lock-screen') !== null`, 15_000);
    await page.sleep(400);
    await shoot("lock");
    await setValue(page, `document.querySelector('.lock-screen input[type=password]')`, "inventory-pw");
    await page.eval(`document.querySelector('.lock-screen button[type=submit]').click()`);
    await page.waitFor(`document.querySelector('.lock-screen') === null`, 15_000);
  } finally {
    await invoke("disable_vault").catch(() => {});
  }
}

async function maskScreens(page, shoot) {
  await openTable(page, "customers");
  await key(page, "m", "KeyM", 77, CTRL | 1);
  try {
    await page.waitFor(`document.querySelector('.topbar-masked') !== null`, 15_000);
    await page.sleep(500);
    await shoot();
  } finally {
    if (await page.eval(`document.querySelector('.topbar-masked') !== null`)) await key(page, "m", "KeyM", 77, CTRL | 1);
  }
}

async function snapshotScreens(page, shoot) {
  await newEditor(page);
  await setEditor(page, "UPDATE products SET price = 25 WHERE id = 4;\nUPDATE products SET price = 24 WHERE id = 4;");
  await page.eval(`(() => { const s = ${queryTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await key(page, "x", "KeyX", 88, 1);
  const button = (label) => `[...${queryTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(tr(label))})`;
  await page.waitFor(`${button("Cancel")}?.disabled && (${queryTab}.querySelector('.statusbar span')?.textContent ?? '') !== ''`, 30_000);
  await page.eval(`${button("Transaction log")}.click()`);
  await page.waitFor(`document.querySelector('.modal.tx-log') !== null`);
  await page.sleep(500);
  await shoot("log");
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`document.querySelector('.tx-log-restore').click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${tabs}`, 15_000);
  await page.sleep(600);
  await shoot("restore");
}

async function agentsScreens(page, shoot) {
  await key(page, ",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.waitFor(`document.querySelector('.agents-panel [data-client]') !== null`);
  const clients = await page.eval(`[...document.querySelectorAll('.agents-panel [data-client]')].map(b => b.dataset.client)`);
  await page.sleep(400);
  await shoot("panel");
  for (const client of clients) {
    await page.eval(`document.querySelector('.agents-panel [data-client=${JSON.stringify(client)}]').click()`);
    await page.sleep(500);
    await shoot(client);
  }
}

async function syncAndSharingScreens(page, shoot) {
  await key(page, ",", "Comma", 188, CTRL);
  await page.settingsTab("Sync");
  await page.waitFor(`document.querySelector('.sync-panel') !== null`);
  await page.sleep(400);
  await shoot("sync");
  await reset(page);
  const r = await page.rect(`.project-square[data-project="${PROJECT}"]`);
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await shoot("project-menu");
  await clickText(page, ".context-menu button", "Share project…");
  await page.waitFor(`document.querySelector('.share-project-dialog') !== null`);
  await page.sleep(300);
  await shoot("share");
  await reset(page);
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog [data-action="open-shared"]') !== null`);
  await shoot("open-shared");
}

async function tasksScreens(page, shoot) {
  await palette(page, "Scheduled tasks");
  await page.waitFor(`document.querySelector('.schedules-dialog') !== null`);
  await page.sleep(500);
  await shoot("empty");
  await clickText(page, ".schedules-dialog button", "New task…");
  await page.waitFor(`document.querySelector('.schedule-form') !== null`);
  await page.sleep(300);
  await shoot("new");
}

async function profilesScreens(page, shoot) {
  await palette(page, "Transfer profiles");
  await page.waitFor(`document.querySelector('.schedules-dialog') !== null`);
  await page.sleep(600);
  await shoot();
}

async function menuSubmenuScreens(page, shoot) {
  await expandShop(page);
  await rightClick(page, "orders");
  await shoot("menu");
  const count = await page.eval(`[...document.querySelectorAll('.context-menu button')].filter(b => b.hasAttribute('aria-haspopup')).length`);
  for (let i = 0; i < count; i++) {
    const label = await page.eval(`[...document.querySelectorAll('.context-menu button')].filter(b => b.hasAttribute('aria-haspopup'))[${i}].querySelector('span')?.textContent ?? ''`);
    if (!(await openSubmenu(page, label))) continue;
    await page.sleep(300);
    await shoot(label);
  }
}

async function pinnedTabScreens(page, shoot) {
  await openTable(page, "customers");
  await openTable(page, "orders");
  const pin = async (label) => {
    await page.eval(`document.querySelectorAll('.tabs .tab')[0].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 60 }))`);
    await page.waitFor(`document.querySelector('.context-menu') !== null`);
    await shoot(`${label}-menu`);
    await page.eval(`[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith(${JSON.stringify(tr(label))})).click()`);
  };
  await pin("Pin tab");
  try {
    await page.waitFor(`document.querySelectorAll('.tabs .tab')[0].classList.contains('pinned')`);
    await page.sleep(300);
    await shoot("pinned");
  } finally {
    await pin("Unpin tab").catch(() => {});
  }
}

async function gridEditingScreens(page, shoot) {
  await openTable(page, "customers");
  await page.eval(`${queryTab}.querySelector('.grid-scroll').focus()`);
  await key(page, "ArrowDown", "ArrowDown", 40);
  await key(page, "ArrowRight", "ArrowRight", 39);
  await key(page, "ArrowRight", "ArrowRight", 39);
  await key(page, "F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.sleep(200);
  await shoot("cell-editor");
  await key(page, "a", "KeyA", 65, CTRL);
  await page.type("Asti");
  await key(page, "Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
  await page.eval(`${queryTab}.querySelector('.edit-bar button[aria-label=${JSON.stringify(tr("Preview SQL"))}]').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  await page.sleep(400);
  await shoot("preview-sql");
  await escape(page);
  await page.eval(`${queryTab}.querySelector('.edit-bar button[aria-label=${JSON.stringify(tr("Add a row"))}]').click()`);
  await page.sleep(500);
  await shoot("new-row");
}

async function columnHoverScreens(page, shoot) {
  await newEditor(page);
  await setEditor(page, "SELECT c.id, city, status, total FROM customers c JOIN orders o ON o.customer_id = c.id");
  await run(page);
  const grid = await page.eval(`(() => { const r = ${queryTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.mouse("mouseMoved", grid.x + 140, grid.y + 14);
  await page.waitFor(`document.querySelector('.column-profile') !== null`, 8_000);
  await page.sleep(300);
  await shoot();
}

async function connectionDialogScreens(page, shoot) {
  await page.eval(`document.querySelector('.sidebar-column button[aria-label=${JSON.stringify(tr("New connection"))}]').click()`);
  await page.waitFor(`document.querySelector('.driver-grid') !== null`);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent.includes('PostgreSQL')).click()`);
  await page.waitFor(`document.querySelector('.modal .dialog-tabs [role=tab]') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal .dialog-tabs [role=tab]')].find(t => t.textContent.includes(${JSON.stringify(tr("Network"))})).click()`);
  await page.sleep(300);
  await page.eval(`[...document.querySelectorAll('.modal label')].find(l => l.textContent.includes(${JSON.stringify(tr("Connect through an SSH tunnel"))})).querySelector('input').click()`);
  await page.waitFor(`document.querySelector('.modal input[placeholder="bastion.example.com"]') !== null`);
  await page.sleep(300);
  await shoot("ssh");
}

async function importConnectionsScreens(page, shoot) {
  await page.eval(`document.querySelector('.sidebar-column button[aria-label=${JSON.stringify(tr("Import connections"))}]').click()`);
  await page.waitFor(`document.querySelector('.modal') !== null`);
  await page.sleep(800);
  await shoot();
}

/** The screens of the UX review, one or more pictures for each section of the group T checklist. */
function moreScreens() {
  return [
    { name: "query-builder", run: builderScreens },
    { name: "notebook", run: notebookScreens },
    { name: "chart-pivot", run: chartPivotScreens },
    { name: "schema-tools", run: schemaToolsScreens },
    { name: "server-admin", run: adminScreens },
    { name: "lock-screen", run: lockScreens },
    { name: "masking", run: maskScreens },
    { name: "snapshots", run: snapshotScreens },
    { name: "agents-mcp", run: agentsScreens },
    { name: "sync-sharing", run: syncAndSharingScreens },
    { name: "scheduled-tasks", run: tasksScreens },
    { name: "transfer-profiles", run: profilesScreens },
    { name: "table-menu-submenu", run: menuSubmenuScreens },
    { name: "pinned-tab", run: pinnedTabScreens },
    { name: "grid-editing", run: gridEditingScreens },
    { name: "column-hover", run: columnHoverScreens },
    { name: "connection-ssh", run: connectionDialogScreens },
    { name: "import-connections", run: importConnectionsScreens },
  ];
}

/** Every page of Settings and, in each, every sub-tab. */
async function settingsScreens(page, shoot) {
  await key(page, ",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings [role=tab][data-section]') !== null`);
  const sections = await page.eval(`[...document.querySelectorAll('.modal.settings .settings-nav [role=tab]')].map(t => t.dataset.section)`);
  for (const section of sections) {
    await page.eval(`document.querySelector('.modal.settings .settings-nav [data-section=${JSON.stringify(section)}]').click()`);
    await page.sleep(400);
    await page.eval(`document.activeElement?.blur()`);
    const subs = await page.eval(`[...document.querySelectorAll('.modal.settings .settings-body .dialog-tabs [role=tab]')].map(t => t.textContent)`);
    const slug = section.toLowerCase().replace(/[^a-z0-9]+/g, "-");
    if (subs.length === 0) {
      await shoot(slug);
      continue;
    }
    for (let i = 0; i < subs.length; i++) {
      await page.eval(`document.querySelectorAll('.modal.settings .settings-body .dialog-tabs [role=tab]')[${i}].click()`);
      await page.sleep(400);
      await shoot(`${slug}-${i + 1}`);
    }
  }
}

/** Walks the screens and saves their pictures for `variant`, e.g. `dark-en`. */
export async function runInventory(page, ctx, base) {
  // PLATYPUS_INVENTORY_SIZE=1280x720 sets the viewport (default 1440x900) and goes in the folder name.
  const given = /^(\d+)x(\d+)$/.exec(process.env.PLATYPUS_INVENTORY_SIZE ?? "");
  const [width, height] = given ? [Number(given[1]), Number(given[2])] : [1440, 900];
  const variant = given ? `${base}-${width}x${height}` : base;
  const out = resolve(ctx.shots ?? join(ctx.dir, "shots"), "inventory", variant);
  mkdirSync(out, { recursive: true });
  // One window size for every variant, so the pictures compare.
  await page.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await page.send("Page.enable");
  await page.eval(`(() => {
    const style = document.createElement("style");
    style.textContent = "input, textarea { caret-color: transparent !important } .cm-cursorLayer { animation: none !important }";
    document.head.append(style);
  })()`);
  await page.sleep(500);
  let n = 0;
  const done = [];
  const missed = [];
  // PLATYPUS_INVENTORY_ONLY=er-diagram,transfer runs just those, to try a screen out.
  const only = (process.env.PLATYPUS_INVENTORY_ONLY ?? "").split(",").filter(Boolean);
  for (const screen of screens.filter((s) => only.length === 0 || only.includes(s.name))) {
    const started = performance.now();
    const shoot = async (part) => {
      n++;
      const file = `${String(n).padStart(2, "0")}-${screen.name}${part ? `-${String(part).normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-")}` : ""}.png`;
      await page.sleep(150);
      await page.screenshot(join(out, file));
      done.push(file);
    };
    try {
      await screen.run(page, shoot);
      console.log(`     inventory ${variant}: ${screen.name} (${Math.round(performance.now() - started)} ms)`);
    } catch (e) {
      missed.push(screen.name);
      console.log(`     inventory ${variant}: SKIPPED ${screen.name}: ${String(e instanceof Error ? e.message : e).split("\n")[0].slice(0, 160)}`);
      await page.screenshot(join(out, `_failed-${screen.name}.png`)).catch(() => {});
    }
    await reset(page);
  }
  console.log(`     inventory ${variant}: ${done.length} pictures, ${missed.length} screens skipped${missed.length ? ` (${missed.join(", ")})` : ""}`);
}
