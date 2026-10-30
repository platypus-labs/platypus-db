// The automatic layout of the ER diagram on a real-looking schema: 57 SQLite tables, with users
// and studies referenced from nearly every table, eight modules of a head table and its
// details, a self reference, a cycle of two tables and three tables alone. On opening, after a
// table is dragged and Auto layout is pressed, and after a table is added outside the app: no
// two boxes overlap, every foreign key has its line, no line runs through a table that is not
// one of its ends, every line is made of level and upright pieces, the hubs sit level with the
// tables referencing them (not at the top of a column), and the tables alone go below the
// others. A screenshot of the whole diagram, in full screen at 50%, is kept with the shots.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../qa3-schema.mjs";

const MODULES = 8;
const DETAILS = 5;

/** The schema: every table with its foreign keys, as `[column, referenced table]`. */
function schema() {
  const tables = [
    { name: "users", keys: [] },
    { name: "studies", keys: [["owner_id", "users"]] },
    { name: "sites", keys: [["study_id", "studies"]] },
    { name: "categories", keys: [["parent_id", "categories"], ["created_by", "users"]] },
    { name: "cyc_a", keys: [["b_id", "cyc_b"], ["study_id", "studies"]] },
    { name: "cyc_b", keys: [["a_id", "cyc_a"]] },
    { name: "audit_log", keys: [] },
    { name: "settings", keys: [] },
    { name: "kv", keys: [] },
  ];
  for (let m = 0; m < MODULES; m++) {
    const head = `m${m}_head`;
    tables.push({ name: head, keys: [["study_id", "studies"], ["created_by", "users"], ...(m % 3 === 0 ? [["site_id", "sites"]] : [])] });
    for (let k = 0; k < DETAILS; k++) {
      const keys = [["head_id", head]];
      if (k % 2 === 0) keys.push(["user_id", "users"]);
      if (k >= 3) keys.push(["previous_id", `m${m}_d${k - 1}`]);
      if (k === 4) keys.push(["study_id", "studies"]);
      if (m === 2 && k === 1) keys.push(["category_id", "categories"]);
      tables.push({ name: `m${m}_d${k}`, keys });
    }
  }
  return tables;
}

const TABLES = schema();
const KEYS = TABLES.reduce((s, t) => s + t.keys.length, 0);
const ALONE = ["audit_log", "settings", "kv"];

/** The CREATE TABLE statements; SQLite takes a reference to a table created later. */
const ddl = (tables) =>
  tables
    .map(({ name, keys }) => {
      const columns = ["id INTEGER PRIMARY KEY", "label TEXT", ...keys.map(([column, ref]) => `${column} INTEGER REFERENCES ${ref}(id)`), "note TEXT"];
      return `CREATE TABLE ${name} (${columns.join(", ")});`;
    })
    .join("\n");

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, ddl(TABLES));
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

/** Every drawn table: its name and box, as the page shows them. */
const boxes = `[...${visibleErd}.querySelectorAll('.erd-table')].map(g => {
  const r = g.querySelector('rect');
  const m = /translate\\(([-\\d.]+),\\s*([-\\d.]+)\\)/.exec(g.getAttribute('transform'));
  return { name: g.querySelector('text').textContent, transform: g.getAttribute('transform'), x: +m[1], y: +m[2], w: +r.getAttribute('width'), h: +r.getAttribute('height') };
})`;

/** Every line of a foreign key: its two tables (from data-fk, "table.columns->target") and the
 *  corners of its path (the control point of a rounded corner is the corner itself). */
const lines = `[...${visibleErd}.querySelectorAll('path[marker-start]')].map(p => {
  const fk = p.getAttribute('data-fk');
  const nums = (p.getAttribute('d').match(/-?[\\d.]+(e-?\\d+)?/g) ?? []).map(Number);
  const points = [];
  for (let i = 0; i + 1 < nums.length; i += 2) points.push({ x: nums[i], y: nums[i + 1] });
  return { from: fk.slice(0, fk.indexOf('.')), to: fk.slice(fk.indexOf('->') + 2), points };
})`;

/** Asserts that no two boxes of the diagram overlap. */
function assertDisjoint(list, what) {
  for (const [i, a] of list.entries()) {
    for (const b of list.slice(i + 1)) {
      const apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
      assert.ok(apart, `${what}: ${a.name} overlaps ${b.name}: ${JSON.stringify([a, b])}`);
    }
  }
}

/** Asserts that every line is made of level and upright pieces (the corners rounded within a few
 *  pixels) and runs through no table but its own two. Returns how many times lines cross. */
function assertRouted(list, drawn, what) {
  const pieces = [];
  for (const line of list) {
    for (const [i, p] of line.points.slice(1).entries()) {
      const q = line.points[i];
      const level = Math.abs(p.y - q.y) < 0.5;
      const upright = Math.abs(p.x - q.x) < 0.5;
      // The two halves of a rounded corner are at most 6 px each way.
      assert.ok(level || upright || (Math.abs(p.x - q.x) <= 6.5 && Math.abs(p.y - q.y) <= 6.5), `${what}: a slanted piece in ${line.from}->${line.to}: ${JSON.stringify([q, p])}`);
      if (!level && !upright) continue;
      pieces.push({ line, q, p, level });
      for (const b of drawn) {
        if (b.name === line.from || b.name === line.to) continue;
        const inside = Math.min(p.x, q.x) < b.x + b.w - 1 && Math.max(p.x, q.x) > b.x + 1 && Math.min(p.y, q.y) < b.y + b.h - 1 && Math.max(p.y, q.y) > b.y + 1;
        assert.ok(!inside, `${what}: the line ${line.from}->${line.to} runs through ${b.name}: ${JSON.stringify([q, p, b])}`);
      }
    }
  }
  const crossings = new Set();
  for (const a of pieces.filter((s) => s.level)) {
    for (const b of pieces.filter((s) => !s.level && s.line !== a.line)) {
      const x = b.q.x;
      const y = a.q.y;
      if (x > Math.min(a.q.x, a.p.x) && x < Math.max(a.q.x, a.p.x) && y > Math.min(b.q.y, b.p.y) && y < Math.max(b.q.y, b.p.y)) crossings.add(`${Math.round(x)},${Math.round(y)}`);
    }
  }
  return crossings.size;
}

/** Asserts that a hub's middle is within the span of the tables referencing it. */
function assertHubCentred(drawn, hub) {
  const byName = Object.fromEntries(drawn.map((b) => [b.name, b]));
  const referencing = TABLES.filter((t) => t.keys.some(([, ref]) => ref === hub)).map((t) => byName[t.name]);
  const middles = referencing.map((b) => b.y + b.h / 2);
  const middle = byName[hub].y + byName[hub].h / 2;
  assert.ok(middle > Math.min(...middles) && middle < Math.max(...middles), `${hub} at ${middle}, outside the tables referencing it: ${Math.min(...middles)}..${Math.max(...middles)}`);
}

/** Checks the whole drawing; returns the crossings. */
async function check(page, what, count = TABLES.length, keys = KEYS) {
  const drawn = await page.eval(boxes);
  assert.equal(drawn.length, count, what);
  assertDisjoint(drawn, what);
  const list = await page.eval(lines);
  assert.equal(list.length, keys, `${what}: a line per foreign key`);
  const crossings = assertRouted(list, drawn, what);
  // The diagram is not one very wide row nor one very tall column.
  const width = Math.max(...drawn.map((b) => b.x + b.w));
  const height = Math.max(...drawn.map((b) => b.y + b.h));
  assert.ok(width / height < 5 && height / width < 5, `${what}: ${width} x ${height}`);
  console.log(`  ${what}: ${drawn.length} tables in ${Math.round(width)} x ${Math.round(height)}, ${list.length} lines crossing ${crossings} times`);
  return drawn;
}

async function drag(page, name, dx, dy) {
  const before = (await page.eval(boxes)).find((b) => b.name === name).transform;
  await page.eval(`[...${visibleErd}.querySelectorAll('.erd-table')].find(g => g.querySelector('text').textContent === ${JSON.stringify(name)}).querySelector('rect').id = 'erd-drag'`);
  const r = await page.rect("#erd-drag");
  await page.eval(`document.getElementById('erd-drag').removeAttribute('id')`);
  await page.mouse("mousePressed", r.x + 20, r.y + 10);
  await page.mouse("mouseMoved", r.x + 20 + dx / 2, r.y + 10 + dy / 2, { buttons: 1 });
  await page.mouse("mouseMoved", r.x + 20 + dx, r.y + 10 + dy, { buttons: 1 });
  await page.mouse("mouseReleased", r.x + 20 + dx, r.y + 10 + dy);
  await page.waitFor(`${boxes}.find(b => b.name === ${JSON.stringify(name)}).transform !== ${JSON.stringify(before)}`, 5000);
}

export default async function (page, { dir, shots }) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  // Under 100 tables: the diagram opens without the table picker.
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === ${TABLES.length}`, 30_000);
  await page.waitFor(`${visibleErd}.querySelectorAll('path[marker-start]').length === ${KEYS}`, 10_000);
  const first = await check(page, "on opening");
  for (const hub of ["users", "studies"]) assertHubCentred(first, hub);
  // The tables alone go below every linked one.
  const linkedBottom = Math.max(...first.filter((b) => !ALONE.includes(b.name)).map((b) => b.y + b.h));
  for (const name of ALONE) assert.ok(first.find((b) => b.name === name).y >= linkedBottom, `${name} is below the linked tables`);

  // A table dragged onto the hub, then Auto layout: everything is placed again, the same way.
  const users = first.find((b) => b.name === "users");
  const dragged = first.find((b) => b.name === "m3_d2");
  await drag(page, "m3_d2", users.x - dragged.x + 10, users.y - dragged.y + 10);
  const moved = (await page.eval(boxes)).find((b) => b.name === "m3_d2").transform;
  await page.eval(`${buttonIn(visibleErd, "Auto layout")}.click()`);
  await page.waitFor(`${boxes}.find(b => b.name === 'm3_d2').transform !== ${JSON.stringify(moved)}`, 5000);
  const again = await check(page, "after Auto layout");
  const byName = Object.fromEntries(again.map((b) => [b.name, b.transform]));
  for (const b of first) assert.equal(byName[b.name], b.transform, `Auto layout places ${b.name} as on opening`);

  // A screenshot of the whole diagram, in full screen at 50%.
  await page.eval(`${buttonIn(visibleErd, "Full screen")}.click()`);
  await page.waitFor(`${visibleErd}.classList.contains('erd-full')`);
  await page.eval(`${buttonIn(visibleErd, "Zoom out")}.click()`);
  await page.eval(`${buttonIn(visibleErd, "Zoom out")}.click()`);
  await page.waitFor(`${visibleErd}.textContent.includes('50%')`);
  if (shots) await page.screenshot(join(shots, "erd-layout-large.png"));
  await page.eval(`${buttonIn(visibleErd, "Zoom in")}.click()`);
  await page.eval(`${buttonIn(visibleErd, "Zoom in")}.click()`);
  await page.waitFor(`${visibleErd}.textContent.includes('100%')`);
  await page.eval(`${buttonIn(visibleErd, "Exit full screen")}.click()`);
  await page.waitFor(`!${visibleErd}.classList.contains('erd-full')`);

  // A table referencing both hubs added outside the app, then Refresh: with nothing placed by
  // hand, the whole diagram is laid out again, still without an overlap or a line through a box.
  ENGINES.sqlite.exec(dir, SRC, `CREATE TABLE late (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id), study_id INTEGER REFERENCES studies(id));`);
  await page.eval(`${buttonIn(visibleErd, "Refresh")}.click()`);
  await page.waitFor(`${visibleErd}.querySelectorAll('.erd-table').length === ${TABLES.length + 1}`, 15_000);
  await page.waitFor(`${visibleErd}.querySelectorAll('path[marker-start]').length === ${KEYS + 2}`, 10_000);
  await check(page, "after a table is added", TABLES.length + 1, KEYS + 2);
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('.notice.error').length`), 0);
}
