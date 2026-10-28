// Conditional formatting of the grid on a throwaway SQLite database: rules on a value, a
// range, NULL and a color scale, for a cell or the whole row; the first that matches wins,
// so moving a rule changes the colors; rules are kept for the table, edited, deleted and
// cleared; a rule that is not complete cannot be added. The grid is a canvas: the colors
// are read from its pixels.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E grid formatting";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const dialog = `document.querySelector('.formatting-dialog')`;
const field = (name) => `${dialog}.querySelector('[data-field=${name}]')`;
const button = (label) =>
  `[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)})`;
const rules = `[...${dialog}.querySelectorAll('.formatting-rule')].map(r => r.querySelector('.formatting-text').textContent + ' · ' + r.querySelector('.formatting-target').textContent)`;

/**
 * How many pixels of a grid row are of each tint, on a line near its top (clear of the
 * text): `red`, `orange`, `green`, `blue`. Grays (the background, the borders) count for none.
 */
const tints = (row) => `(() => {
  const g = document.getElementById('e2e-grid').closest('.grid');
  const c = g.querySelector('canvas');
  const dpr = window.devicePixelRatio || 1;
  const rowH = Number(getComputedStyle(g).getPropertyValue('--grid-row-h')) || 24;
  const y = Math.round((40 + ${row} * rowH + 3) * dpr);
  const data = c.getContext('2d').getImageData(0, y, c.width, 1).data;
  const out = { red: 0, orange: 0, green: 0, blue: 0 };
  for (let i = 0; i < data.length; i += 4) {
    const [r, gr, b] = [data[i], data[i + 1], data[i + 2]];
    if (Math.max(r, gr, b) - Math.min(r, gr, b) < 20) continue;
    if (r > gr && r > b && Math.abs(gr - b) < 12) out.red++;
    else if (r > gr && gr > b) out.orange++;
    else if (gr >= r && gr >= b) out.green++;
    else if (b > r && b >= gr) out.blue++;
  }
  return out;
})()`;

export function setup({ dir }) {
  const path = join(dir, "formatting.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, customer TEXT, status TEXT, amount REAL, shipped_at TEXT);
           INSERT INTO orders VALUES
             (1, 'Ada', 'paid', 20, '2026-10-01'),
             (2, 'Bob', 'late', 35, '2026-10-02'),
             (3, 'Cy', 'paid', 150, NULL),
             (4, 'Dee', 'refunded', 480, '2026-10-04'),
             (5, 'Eve', 'late', 220, '2026-10-05'),
             (6, 'Fay', 'paid', 5, '2026-10-06');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-0000000b0001"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function openTable(page) {
  await page.waitFor(`${treeRow("orders")} !== undefined`);
  await page.eval(`${treeRow("orders")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${status} === '6 rows'`, 15_000);
  await page.eval(`document.querySelectorAll('#e2e-grid').forEach(e => e.removeAttribute('id'))`);
  await page.eval(`${grid}.id = 'e2e-grid'`);
}

/** Focuses the cell `col` display positions right of the first column of `row`. */
async function focusCell(page, row, col) {
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

/** Opens the rules from the grid's menu, on the focused cell's column. */
async function openRules(page) {
  const r = await page.rect(`#e2e-grid`);
  await page.click(r.x + 4, r.y + 52, { button: "right" });
  await page.waitFor(`${menuButton("Conditional formatting…")} !== undefined`);
  await page.eval(`${menuButton("Conditional formatting…")}.click()`);
  await page.waitFor(`${dialog} !== null`);
}

/** Sets a select or an input of the rule's fields as typing or picking would. */
async function set(page, name, value) {
  await page.eval(`(() => {
    const el = ${field(name)};
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  })()`);
  await page.waitFor(`${field(name)}?.value === ${JSON.stringify(value)}`);
}

async function color(page, swatches, name) {
  await page.eval(`${field(swatches)}.querySelector('[data-color=${name}]').click()`);
  await page.waitFor(`${field(swatches)}.querySelector('[data-color=${name}]').getAttribute('aria-checked') === 'true'`);
}

async function addRule(page, count) {
  await page.eval(`${dialog}.querySelector('[data-action=save-rule]').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.formatting-rule').length === ${count}`);
}

async function close(page) {
  await page.eval(`${button("Close")}.click()`);
  await page.waitFor(`${dialog} === null`);
}

/** Waits for row `row`'s tints to meet `test`, e.g. `t => t.orange > 50`. */
async function waitTint(page, row, test, what) {
  try {
    await page.waitFor(`(${test})(${tints(row)})`);
  } catch (e) {
    throw new Error(`${what}: row ${row} is ${JSON.stringify(await page.eval(tints(row)))}`, { cause: e });
  }
}

const none = "t => t.red + t.orange + t.green + t.blue === 0";

export default async function (page, { shots }) {
  const shot = async (name) => shots && (await page.screenshot(join(shots, `grid-formatting-${name}.png`)));
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await openTable(page);
  // The last row keeps the focus while colors are read: the selection tints its cells.
  await focusCell(page, 5, 2);

  // A rule on the focused column: `status` = late colors the whole row.
  await openRules(page);
  assert.equal(await page.eval(`${field("column")}.value`), "status");
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.formatting-rule').length`), 0);
  assert.ok(
    !(await page.eval(`[...${field("condition")}.options].some(o => o.value === 'scale')`)),
    "no color scale on a text column",
  );
  assert.ok(await page.eval(`${dialog}.querySelector('[data-action=save-rule]').disabled`), "no value, no rule");
  await set(page, "value", "late");
  await set(page, "target", "row");
  await color(page, "color", "orange");
  await addRule(page, 1);

  // A range on a number column: not a number is refused, then `amount` ≥ 100 in green.
  await set(page, "column", "amount");
  await set(page, "condition", "between");
  await set(page, "min", "lots");
  assert.ok(await page.eval(`${dialog}.querySelector('[data-action=save-rule]').disabled`), "a bound must be a number");
  await set(page, "min", "100");
  await set(page, "target", "cell");
  await color(page, "color", "green");
  await addRule(page, 2);
  assert.deepEqual(await page.eval(rules), ["status = late · Row", "amount ≥ 100 · Cell"]);
  await shot("dialog");
  await close(page);

  // Late rows are orange from end to end, `late` before the range: Eve's 220 too.
  await waitTint(page, 1, "t => t.orange > 120 && t.green === 0", "Bob is late");
  await waitTint(page, 4, "t => t.orange > 120 && t.green === 0", "Eve is late, first rule wins");
  await waitTint(page, 2, "t => t.green > 20 && t.orange === 0", "Cy's 150");
  await waitTint(page, 3, "t => t.green > 20 && t.orange === 0", "Dee's 480");
  await waitTint(page, 0, none, "Ada");
  await shot("rules");

  // Moving the range first: Eve's amount turns green, the rest of her row stays orange.
  await openRules(page);
  await page.eval(`${dialog}.querySelectorAll('.formatting-rule')[1].querySelector('button[title="Move up"]').click()`);
  await page.waitFor(`${rules}[0] === 'amount ≥ 100 · Cell'`);
  await close(page);
  await waitTint(page, 4, "t => t.green > 20 && t.orange > 80", "Eve, range first");

  // Kept for the table: closed and opened again, the same rules color the same rows.
  await page.key("w", "KeyW", 87, CTRL);
  await openTable(page);
  await focusCell(page, 5, 4);
  await waitTint(page, 4, "t => t.green > 20 && t.orange > 80", "Eve, after reopening");
  await waitTint(page, 0, none, "Ada, after reopening");

  // NULL in blue, on the focused column `shipped_at`.
  await openRules(page);
  assert.equal(await page.eval(`${field("column")}.value`), "shipped_at");
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.formatting-rule').length`), 2);
  await set(page, "condition", "null");
  await color(page, "color", "blue");
  await addRule(page, 3);

  // Editing a rule: `late` becomes `paid`.
  await page.eval(`[...${dialog}.querySelectorAll('.formatting-text')].find(b => b.textContent === 'status = late').click()`);
  await page.waitFor(`${button("Update rule")} !== undefined && ${field("value")}.value === 'late'`);
  await set(page, "value", "paid");
  await page.eval(`${dialog}.querySelector('[data-action=save-rule]').click()`);
  await page.waitFor(`${rules}.includes('status = paid · Row')`);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.formatting-rule').length`), 3, "edited in place");
  await close(page);
  await waitTint(page, 0, "t => t.orange > 120", "Ada is paid");
  await waitTint(page, 1, none, "Bob is no longer colored");
  // Cy: 150 in green; the row rule comes before NULL, so the date is orange too.
  await waitTint(page, 2, "t => t.green > 20 && t.orange > 100 && t.blue === 0", "Cy, paid before NULL");
  await openRules(page);
  await page.eval(`${dialog}.querySelectorAll('.formatting-rule')[2].querySelector('button[title="Move up"]').click()`);
  await page.waitFor(`${rules}[1] === 'shipped_at is NULL · Cell'`);
  await close(page);
  await waitTint(page, 2, "t => t.blue > 20 && t.green > 20 && t.orange > 50", "Cy, NULL before paid");

  // A color scale from red (the lowest amount) to green (the highest), alone.
  await openRules(page);
  await page.eval(`${button("Clear all")}.click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.formatting-rule').length === 0`);
  await set(page, "column", "amount");
  await set(page, "condition", "scale");
  await color(page, "color", "red");
  await color(page, "to", "green");
  await addRule(page, 1);
  assert.deepEqual(await page.eval(rules), ["amount: color scale · Cell"]);
  await close(page);
  await waitTint(page, 3, "t => t.green > 20 && t.red === 0", "Dee's 480 is the highest");
  await focusCell(page, 0, 0);
  await waitTint(page, 5, "t => t.red > 20 && t.green === 0", "Fay's 5 is the lowest");
  await shot("scale");

  // Deleting the last rule leaves the grid as it was, also when opened again.
  await openRules(page);
  await page.eval(`${dialog}.querySelector('.formatting-rule button[title="Delete rule"]').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.formatting-rule').length === 0 && ${dialog}.querySelector('.formatting-empty') !== null`);
  await close(page);
  for (const row of [3, 5]) await waitTint(page, row, none, "no rules");
  await page.key("w", "KeyW", 87, CTRL);
  await openTable(page);
  await focusCell(page, 0, 0);
  await openRules(page);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.formatting-rule').length`), 0);
  await close(page);
}
