// The look of the main screens in Platypus Dark and Light: the project rail, navigator with a
// table open, editor with results, palette and Settings. With PLATYPUS_E2E_SHOTS set it saves
// visual-<theme>-<screen>.png, to compare with the design by eye.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "Shop dev";
const cssVar = (name) => `getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "shop.db");
  const db = new DatabaseSync(path);
  db.exec(`
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id),
      placed_at TEXT, status TEXT, total NUMERIC, note TEXT);
    CREATE VIEW paid_orders AS SELECT * FROM orders WHERE status = 'paid';
    INSERT INTO customers VALUES (1, 'Giulia Ferrari', 'Milano'), (2, 'Marco Conti', NULL), (3, 'Sara Greco', 'Udine');
    INSERT INTO orders VALUES
      (2981, 1, '2026-10-11 18:02', 'paid', 1284.5, NULL),
      (2974, 2, '2026-10-10 09:41', 'paid', 742, 'Leave at the door'),
      (2966, 3, '2026-10-09 21:15', 'shipped', 913.2, NULL);
  `);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "00000000-0000-4000-9000-000000000011"
name = "Negozio"
color = "#d97706"
connections = ["00000000-0000-4000-8000-000000000011"]

[[projects]]
id = "00000000-0000-4000-9000-000000000012"
name = "Interno"
color = "#0d9488"

[[connections]]
id = "00000000-0000-4000-8000-000000000011"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
connection_type = "dev"
`,
  );
  writeFileSync(join(dir, "settings.toml"), '[appearance]\ntheme = "platypus_dark"\n');
}

async function screens(page, theme, shots) {
  const shot = (name) => shots && page.screenshot(join(shots, `visual-${theme}-${name}.png`));
  await page.waitFor(`document.fonts.status === 'loaded'`);
  // The frame fades to the new theme's colors.
  await page.waitFor(`document.getAnimations().length === 0`);
  await shot("workspace");

  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type("ord");
  await page.waitFor(`document.querySelector('.palette li.selected') !== null`);
  await shot("palette");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  // Theme cards are buttons taller than the others: their color strip stays inside them.
  const strip = await page.eval(`(() => {
    const card = document.querySelector('.theme-card').getBoundingClientRect();
    const colors = document.querySelector('.theme-card .theme-card-colors').getBoundingClientRect();
    return colors.height > 0 && colors.bottom <= card.bottom;
  })()`);
  assert.equal(strip, true, "theme card colors inside the card");
  await shot("settings");
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

export default async function (page, { shots }) {
  await page.waitFor(`${cssVar("--bg")} === '#16191b'`);
  // All connections, then the two projects (they load after the theme); Negozio becomes the
  // workspace shown.
  await page.waitFor(`document.querySelectorAll('.project-square').length === 3`);
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.project-square')].map(s => s.title)`),
    ["All connections", "Negozio", "Interno"],
  );
  // No tab yet: the welcome screen, with its actions and the shell's shortcuts.
  assert.deepEqual(
    JSON.parse(await page.eval(`JSON.stringify([...document.querySelectorAll('.welcome-card strong')].map(s => s.textContent))`)),
    ["Open a data file", "Import connections"],
  );
  assert.equal(await page.eval(`document.querySelector('.welcome-keys kbd')?.textContent`), "Ctrl+K");
  await page.key("2", "Digit2", 50, CTRL);
  await page.waitFor(`document.querySelector('.navigator-project')?.textContent === 'Negozio'`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("orders")} !== undefined`, 15_000);
  await page.eval(`${treeRow("orders")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '3 rows'`, 15_000);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.conn-chip.dev')?.textContent`), "Shop dev · main");
  // The frame takes the DEV tint; the top bar names the project, the connection and its type.
  assert.equal(await page.eval(`document.querySelector('.shell').dataset.env`), "dev");
  assert.equal(await page.eval(`getComputedStyle(document.querySelector('.shell')).getPropertyValue('--frame').trim()`), "#0f1513");
  assert.equal(await page.eval(`document.querySelector('.topbar-project').textContent`), "Negozio");
  assert.equal(await page.eval(`document.querySelector('.topbar-connection').textContent`), "Shop devDevelopment");
  await screens(page, "dark", shots);

  // The sidebar button: Platypus Dark, then System, then Platypus Light.
  const toggle = `document.querySelector('button[title^="Theme:"]')`;
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${toggle}.title.startsWith('Theme: System')`);
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${cssVar("--bg")} === '#ffffff'`);
  assert.equal(await page.eval(cssVar("--accent")), "#b85600");
  await screens(page, "light", shots);
}
