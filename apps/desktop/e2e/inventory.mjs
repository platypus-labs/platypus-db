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
import { setField } from "./plans.mjs";

const here = dirname(fileURLToPath(import.meta.url));

export const NAME = "Shop dev";
const ARCHIVE = "Archive db";
const PROJECT = "00000000-0000-4000-9000-0000000a0031";
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
];

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
export async function runInventory(page, ctx, variant) {
  const out = resolve(ctx.shots ?? join(ctx.dir, "shots"), "inventory", variant);
  mkdirSync(out, { recursive: true });
  // One window size for every variant, so the pictures compare.
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
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
