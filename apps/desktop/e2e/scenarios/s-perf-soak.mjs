// Group S, performance: a long session. Cycles of the work a person does all day -- open an editor,
// run a query of 100,000 rows and scroll it, open a table's data, edit a cell and discard the
// edit, chart the result, close every tab, open and close Settings -- repeated for
// PLATYPUS_SOAK_MINUTES minutes (4 by default, so a normal run stays short; 60 for the real
// soak). After each cycle the private memory of the app and of its WebView2 processes, the
// JavaScript heap and the DOM node count go to target/perf/soak-<timestamp>.csv.
//
// A leak shows as steady growth, so the scenario checks two things after the warm-up (the
// first quarter of the run, at least 90 s, when caches, the JIT and the allocator reach their working size):
//   - the memory at the end (median of the last three cycles) is at most GROWTH_MB above the
//     warm-up level (median of its last three cycles);
//   - the least-squares slope of the memory over the cycles after the warm-up, in MB per hour,
//     is at most SLOPE_MB_PER_HOUR (checked only when that span lasts 10 minutes or more:
//     over a few minutes the allocator's noise, +-20 MB, would be extrapolated into hundreds).
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { expand, memory, openEditor, run, saveConnections, scrollFrames, sqliteEntry, treeRow, visibleTab } from "../perf.mjs";
import { editCell, focusCell } from "../qa-grid.mjs";

const NAME = "S soak";
const MINUTES = Number(process.env.PLATYPUS_SOAK_MINUTES ?? 4);
const ROWS = 100_000;
/** Allowed growth of the memory over the warm-up level (MB), and of its slope after it. */
const GROWTH_MB = 150;
const SLOPE_MB_PER_HOUR = 60;
const here = dirname(fileURLToPath(import.meta.url));

export function setup({ dir }) {
  const file = join(dir, "soak.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE big (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT, amount REAL, note TEXT);
    CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, qty INTEGER);
    WITH RECURSIVE i(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM i WHERE g < 200) INSERT INTO items SELECT g, 'item ' || g, g * 3 FROM i;
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < ${ROWS})
    INSERT INTO big SELECT g, g % 40, 'name ' || g, (g * 7919 % 10007) / 7.0, 'a note of some length for row ' || g FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

const tabCount = `document.querySelectorAll('.tab').length`;
const view = (name) => `[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === ${JSON.stringify(name)})`;

/** One round of work. */
async function cycle(page, n) {
  // An editor with a 100,000-row result, loaded whole and scrolled.
  await openEditor(page, NAME);
  await run(page, "SELECT * FROM big");
  await page.eval(`[...${visibleTab}.querySelectorAll(".statusbar button")].find(b => b.textContent === "Fetch all")?.click()`);
  await page.waitFor(`${visibleTab}.querySelector(".statusbar span")?.textContent === "100,000 rows"`, 60_000);
  await scrollFrames(page, `${visibleTab}.querySelector('.grid-scroll')`, 2400, 1200);

  // The same result as a chart, and back.
  await page.eval(`${view("Chart")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.chart-svg') != null`, 30_000);
  await page.eval(`${view("Grid")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.chart-svg') == null && ${visibleTab}.querySelector('.grid-scroll') != null`, 30_000);

  // A table's data from the navigator: edit a cell, then discard the edit.
  await page.eval(`${treeRow("items")}.id = 's-items'; document.getElementById('s-items').scrollIntoView({ block: 'nearest' })`);
  const tabs = await page.eval(tabCount);
  await page.clickSel("#s-items", { clickCount: 2 });
  await page.waitFor(`${tabCount} > ${tabs} && ${visibleTab}?.querySelector('.viewer-bar') != null && /^200 rows?$/.test(${visibleTab}.querySelector('.statusbar span')?.textContent ?? '')`, 30_000);
  await page.eval(`document.getElementById('s-items').removeAttribute('id')`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await focusCell(page, 3, 1);
  await editCell(page, `soak ${n}`);
  await page.waitFor(`${visibleTab}.querySelector('.edit-bar button[aria-label="Discard"]') != null`, 10_000);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Discard"]').click()`);
  await page.waitFor(`${visibleTab}.querySelector('.edit-bar button[aria-label="Discard"]') == null`, 10_000);

  // Every tab closed (an editor with text asks whether to keep it).
  await closeTabs(page);

  // A dialog opened and closed.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') != null`, 10_000);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') == null`, 10_000);
}

async function closeTabs(page) {
  for (let i = 0; i < 20 && (await page.eval(tabCount)) > 0; i++) {
    const before = await page.eval(tabCount);
    await page.key("w", "KeyW", 87, CTRL);
    await page.waitFor(
      `${tabCount} < ${before} || document.querySelector('.modal.confirm') != null`,
      10_000,
    );
    // Not saving the script is the answer of someone who closes a scratch editor.
    await page.eval(`(() => {
      const m = document.querySelector('.modal.confirm');
      const b = m && [...m.querySelectorAll('button')].find(b => /^(Don.t save|Discard|Close|No)/i.test(b.textContent.trim()));
      if (b) b.click();
    })()`);
  }
  await page.waitFor(`${tabCount} === 0`, 10_000);
}

const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

/** Least-squares slope of `ys` over `ts` (seconds), in units per hour. */
function slopePerHour(ts, ys) {
  const n = ts.length;
  const mt = ts.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  const num = ts.reduce((s, t, i) => s + (t - mt) * (ys[i] - my), 0);
  const den = ts.reduce((s, t) => s + (t - mt) ** 2, 0);
  return den === 0 ? 0 : (num / den) * 3600;
}

export default async function (page, { dir }) {
  const csvDir = join(here, "../../../../target/perf");
  mkdirSync(csvDir, { recursive: true });
  const csv = join(csvDir, `soak-${new Date().toISOString().replace(/[:.]/g, "-")}.csv`);
  const lines = ["cycle,seconds,app_mb,webview_mb,total_mb,js_heap_mb,dom_nodes"];
  const samples = [];
  const started = Date.now();
  const end = started + MINUTES * 60_000;

  await expand(page, NAME);
  await page.waitFor(`${treeRow("items")} !== undefined`, 15_000);
  const sample = async (n) => {
    const m = memory(dir);
    const heap = await page.eval(`Math.round((performance.memory?.usedJSHeapSize ?? 0) / 1e6)`);
    const nodes = await page.eval(`document.getElementsByTagName('*').length`);
    const seconds = Math.round((Date.now() - started) / 1000);
    samples.push({ n, seconds, total: m.app + m.webview });
    lines.push(`${n},${seconds},${m.app},${m.webview},${m.app + m.webview},${heap},${nodes}`);
    writeFileSync(csv, `${lines.join("\n")}\n`);
    console.log(`  cycle ${n} (${seconds} s): app ${m.app} + webview ${m.webview} = ${m.app + m.webview} MB, JS heap ${heap} MB, ${nodes} DOM nodes`);
  };

  await sample(0);
  let n = 0;
  // At least 8 cycles, so the warm-up and the rest each have enough to compare.
  while (Date.now() < end || n < 8) {
    n++;
    await cycle(page, n);
    await sample(n);
  }
  console.log(`  memory log: ${csv}`);

  // Warm-up: the first quarter of the run, at least 90 s (WebView2 takes about two minutes to
  // reach the memory it keeps), at most the first half.
  const total = (Date.now() - started) / 1000;
  const warmEnd = Math.min(Math.max(total / 4, 90), total / 2);
  const warm = samples.filter((s) => s.seconds <= warmEnd);
  const after = samples.filter((s) => s.seconds > warmEnd);
  assert.ok(warm.length >= 2 && after.length >= 3, `${warm.length} warm-up and ${after.length} later samples`);
  const level = median(warm.slice(-3).map((s) => s.total));
  const last = median(samples.slice(-3).map((s) => s.total));
  const span = after.at(-1).seconds - after[0].seconds;
  const slope = slopePerHour(after.map((s) => s.seconds), after.map((s) => s.total));
  console.log(`  ${n} cycles in ${Math.round(total)} s; warm-up level ${level} MB, end ${last} MB (${last - level >= 0 ? "+" : ""}${last - level} MB), slope ${Math.round(slope)} MB/hour over ${span} s`);
  assert.ok(last - level <= GROWTH_MB, `the memory grew ${last - level} MB over the warm-up level (${level} -> ${last} MB); see ${csv}`);
  if (span >= 600) assert.ok(slope <= SLOPE_MB_PER_HOUR, `the memory grows ${Math.round(slope)} MB/hour; see ${csv}`);
}
