// Regression guard (passes): the ER diagram of a large SQLite schema, 160 tables in a chain of
// foreign keys: the table picker asked first (over 100 tables), its filter, Clear shown and
// Select shown, every table and line drawn within 10 s, zoom halving the drawing, the diagram
// saved as SVG and as PNG (a drawing tens of thousands of pixels wide).
import assert from "node:assert/strict";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../qa3-schema.mjs";
import { CTRL } from "../cdp.mjs";

const N = 160;
const name = (i) => `t_${String(i).padStart(3, "0")}`;
const picker = `document.querySelector('.modal.erd-tables')`;

export function setup({ dir }) {
  const sql = Array.from({ length: N }, (_, k) => {
    const i = k + 1;
    return i === 1
      ? `CREATE TABLE ${name(i)} (id INTEGER PRIMARY KEY, label TEXT);`
      : `CREATE TABLE ${name(i)} (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES ${name(i - 1)}(id), label TEXT);`;
  }).join("\n");
  ENGINES.sqlite.seed(dir, SRC, sql);
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

async function written(page, path) {
  for (let i = 0; i < 150 && !(existsSync(path) && statSync(path).size > 0); i++) await page.sleep(100);
  return existsSync(path) && statSync(path).size > 0;
}

export default async function (page, { dir }) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  // Over 100 tables: the picker comes first.
  await page.waitFor(`${picker} !== null`, 20_000);
  assert.equal(await page.eval(`${picker}.querySelectorAll('.erd-table-list li').length`), N);
  await page.eval(`${picker}.querySelector('input:not([type=checkbox])').focus()`);
  await page.type("t_15");
  await page.waitFor(`${picker}.querySelectorAll('.erd-table-list li').length === 10`);
  await page.eval(`${buttonIn(picker, "Clear shown")}.click()`);
  await page.waitFor(`${picker}.textContent.includes('${N - 10} tables chosen')`);
  await page.eval(`${picker}.querySelector('input:not([type=checkbox])').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("Backspace", "Backspace", 8);
  await page.waitFor(`${picker}.querySelectorAll('.erd-table-list li').length === ${N}`);
  await page.eval(`${buttonIn(picker, "Select shown")}.click()`);
  await page.waitFor(`${picker}.textContent.includes('${N} tables chosen')`);
  const started = Date.now();
  await page.eval(`${buttonIn(picker, "Show")}.click()`);
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === ${N}`, 30_000);
  const took = Date.now() - started;
  console.log(`  ${N} tables drawn in ${took} ms`);
  assert.ok(took < 10_000, `${N} tables took ${took} ms`);
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('path[marker-start]').length`), N - 1);

  // Zoom: 100% → 50%, the drawing halves.
  const width = await page.eval(`Number(${visibleErd}.querySelector('.erd-canvas svg').getAttribute('width'))`);
  await page.eval(`${buttonIn(visibleErd, "Zoom out")}.click()`);
  await page.eval(`${buttonIn(visibleErd, "Zoom out")}.click()`);
  await page.waitFor(`${visibleErd}.textContent.includes('50%')`);
  const half = await page.eval(`Number(${visibleErd}.querySelector('.erd-canvas svg').getAttribute('width'))`);
  assert.ok(Math.abs(half - width / 2) < 2, `zoom 50%: ${width} → ${half}`);

  const svg = join(dir, "large.svg");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(svg)})`);
  await page.eval(`${buttonIn(visibleErd, "SVG")}.click()`);
  assert.ok(await written(page, svg), "SVG written");
  assert.ok(readFileSync(svg, "utf8").includes(name(N)));

  const png = join(dir, "large.png");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(png)})`);
  await page.eval(`${buttonIn(visibleErd, "PNG")}.click()`);
  const ok = await written(page, png);
  const shown = await page.eval(`[...document.querySelectorAll('.toast, .notice.error')].map(e => e.textContent)`);
  assert.ok(ok, `PNG not written for a diagram ${width} px wide; shown: ${JSON.stringify(shown)}`);
  const bytes = readFileSync(png);
  assert.deepEqual([...bytes.subarray(0, 4)], [0x89, 0x50, 0x4e, 0x47]);
  assert.ok(bytes.length > 10_000, `PNG of ${bytes.length} bytes: empty drawing`);
}
