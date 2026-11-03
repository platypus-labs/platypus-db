// Group S, performance: a PostgreSQL table of 5,000,000 rows (s_perf_5m, made once with
// generate_series and reused while it is there) opened as table data from the navigator and as a
// query. Timed and bounded, with the memory of the app after each step: first rows on screen,
// the row count, Ctrl+End (the grid loads blocks as it goes, up to the memory limit of 512 MB
// of buffered rows), a sort and a filter from the grid, the scroll frame rate, and the cancel
// of a long query. The numbers are printed; the bounds hold on the dev machine (Ryzen 7 9700X)
// with a wide margin, since the CI-like runners are several times slower.
// Needs PostgreSQL (docker compose -f docker/compose.yml up -d).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { PG, clipboardAfter, fetchAll, grid, memoryNow, needsPort, openEditor, pgEntry, run, saveConnections, scrollFrames, status, statusbar, timed, visibleTab } from "../perf.mjs";
import { clearStatus, focusCell, gridMenu } from "../qa-grid.mjs";
import { connectWith, openPath, psql } from "../qa-jrn.mjs";

export const skip = needsPort(PG.port, "PostgreSQL");

const TABLE = "s_perf_5m";
const ROWS = 5_000_000;
const rowsText = (n) => new RegExp(`^${String(n).replace(/\B(?=(\d{3})+(?!\d))/g, "[.,]?")} rows?$`);
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
/** The first rows of a streamed result: "1,000 rows loaded · more available" or a whole short one. */
const firstRows = `/^[1-9][\\d.,]* rows?( loaded.*)?$/.test(${status}.trim()) && ${visibleTab}.querySelector('.grid-scroll') !== null`;

export function setup({ dir }) {
  saveConnections(dir, pgEntry());
  const exists = psql(`SELECT to_regclass('public.${TABLE}') IS NOT NULL`) === "t";
  const have = exists ? psql(`SELECT count(*) FROM public.${TABLE}`) : 0;
  if (Number(have) === ROWS) return;
  console.log(`  creating ${TABLE} (${ROWS} rows), once`);
  psql(`DROP TABLE IF EXISTS public.${TABLE};
    CREATE TABLE public.${TABLE} AS SELECT g::bigint AS id, (g % 1000)::int AS grp, 'name ' || g AS name,
      round((g * 7919 % 100003) / 7.0, 2) AS amount, timestamptz '2026-01-01' + g * interval '1 second' AS created, (g % 2 = 0) AS flag
      FROM generate_series(1::bigint, ${ROWS}) g;
    ALTER TABLE public.${TABLE} ADD PRIMARY KEY (id); ANALYZE public.${TABLE}`);
}

export default async function (page, { dir }) {
  const memory = {};
  const mem = (label) => (memory[label] = memoryNow(dir, label));
  await connectWith(page, PG.name);
  await openPath(page, [PG.name, "public", "Tables"], TABLE);
  mem("idle, connected");

  // 1. The table's data, from the navigator: the first rows come before the rest is read.
  let ms = await timed(page, null, "true");
  await page.eval(`document.querySelectorAll('#s-table').forEach(e => e.removeAttribute('id'));
    [...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(TABLE)}).id = 's-table'`);
  const started = performance.now();
  await page.clickSel("#s-table", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null && ${firstRows}`, 60_000);
  ms = Math.round(performance.now() - started);
  console.log(`  table data, first rows on screen: ${ms} ms (${await page.eval(status)})`);
  assert.ok(ms < 1500, `first rows took ${ms} ms`);
  mem("table data, first rows");

  // 2. Count rows (the viewer's button) over the 5M rows.
  ms = await timed(page, `${visibleTab}.querySelector('.viewer-bar button[aria-label="Count rows"]').click()`,
    `/^5[.,]?000[.,]?000 rows$/.test(${visibleTab}.querySelector('.viewer-bar .muted')?.textContent ?? '')`, 60_000);
  console.log(`  count rows: ${ms} ms`);
  assert.ok(ms < 2000, `count took ${ms} ms`);

  // 3. Scrolling the streamed result: blocks are fetched as the grid nears its end.
  let frames = await scrollFrames(page, grid, 480, 3000);
  console.log(`  scrolling the table: ${frames.fps} fps, p95 frame ${frames.p95} ms, longest ${frames.max} ms, down to ${Math.round(frames.top)} px`);
  assert.ok(frames.fps >= 40 && frames.max < 250, `scrolling: ${JSON.stringify(frames)}`);
  mem("after scrolling");

  // 4. "Fetch all" reads the rest of the rows (up to the memory limit), then Ctrl+End goes to the
  // last row.
  await clearStatus(page);
  const ctrlEnd = performance.now();
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  await page.waitFor(`/^[\\d.,]+ rows?$|memory limit/.test(${status}.trim())`, 300_000);
  await focusCell(page, 0, 0); // Ctrl+End needs a focused cell (see qa-perf-ctrl-end-no-focus)
  await page.key("End", "End", 35, CTRL);
  await page.waitFor(`!/loaded/.test(${status})`, 60_000);
  const endStatus = await page.eval(status);
  console.log(`  Fetch all to the end: ${Math.round(performance.now() - ctrlEnd)} ms, status "${endStatus}"`);
  assert.ok(performance.now() - ctrlEnd < 20_000, `fetching all 5M rows took ${Math.round(performance.now() - ctrlEnd)} ms`);
  await page.key("Home", "Home", 36);
  const lastId = await clipboardAfter(page, "c", "KeyC", 67, CTRL);
  console.log(`  last row id: ${lastId}`);
  assert.ok(/memory limit/.test(endStatus) || rowsText(ROWS).test(endStatus.trim()), `end status: ${endStatus}`);
  // The table is read without ORDER BY: a concurrent scan (the row count) makes PostgreSQL start
  // mid-table, so the last row is not id 5000000, but it must be a real, loaded row.
  assert.ok(Number(lastId) >= 1 && Number(lastId) <= ROWS, `the last row (Ctrl+End) is loaded: ${lastId}`);
  const full = mem("after Ctrl+End (rows buffered)");
  assert.ok(full < 1200, `${full} MB with the table buffered`);

  // 5. Sort from the header on the primary key (index scan) and back, then Ctrl+Home.
  frames = null;
  const header = async () => {
    const r = await page.eval(`(() => { const r = ${grid}.getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
    await clearStatus(page);
    await page.click(r.x + 100, r.y + 14);
    return timed(page, null, `${firstRows} && ${cancel}?.disabled === true`, 60_000);
  };
  ms = await header();
  console.log(`  sort by id ascending (header): ${ms} ms`);
  assert.ok(ms < 1000, `sort took ${ms} ms`);
  ms = await header();
  console.log(`  sort by id descending (header): ${ms} ms`);
  assert.ok(ms < 1000, `descending sort took ${ms} ms`);
  mem("after the sorts");

  // 6. Filter from the grid menu: `grp = …` reads the table once and keeps 5,000 rows.
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  await focusCell(page, 0, 1);
  await clearStatus(page);
  ms = await timed(page, null, "true");
  const filterStarted = performance.now();
  await gridMenu(page, "Filter", "Equal to this value");
  await page.waitFor(`${status}.trim() !== '' && !${status}.includes('Executing') && ${cancel}?.disabled === true`, 60_000);
  await fetchAll(page); // the whole filtered result: a stream of 1,000-row blocks otherwise
  ms = Math.round(performance.now() - filterStarted);
  const filtered = await page.eval(status);
  console.log(`  filter grp = …: ${ms} ms, "${filtered}"`);
  assert.match(filtered, /^[\d.,]+ rows?$/);
  assert.ok(Number(filtered.replace(/[^\d]/g, "")) > 1000 && Number(filtered.replace(/[^\d]/g, "")) < 20_000, `filtered: ${filtered}`);
  assert.ok(ms < 2000, `filter took ${ms} ms`);
  mem("after the filter");

  // 7. As a query, in an editor: the first rows, a sort of the whole table on a column with no
  // index, and the cancel of a query that would never end.
  await openEditor(page, PG.name);
  ms = await queryTime(page, `SELECT * FROM ${TABLE}`);
  console.log(`  query SELECT *, first rows: ${ms} ms`);
  assert.ok(ms < 1000, `query first rows took ${ms} ms`);
  ms = await queryTime(page, `SELECT * FROM ${TABLE} ORDER BY amount DESC, id`);
  console.log(`  query ORDER BY amount (5M rows sorted by the server): ${ms} ms`);
  assert.ok(ms < 6000, `sort took ${ms} ms`);
  frames = await scrollFrames(page, grid, 480, 3000);
  console.log(`  scrolling the query: ${frames.fps} fps, p95 frame ${frames.p95} ms, longest ${frames.max} ms`);
  assert.ok(frames.fps >= 40 && frames.max < 250, `scrolling: ${JSON.stringify(frames)}`);
  mem("after the sorted query");

  await run(page, "SELECT 1");
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(`SELECT count(*) FROM ${TABLE} a, ${TABLE} b, ${TABLE} c`);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes('5m c')`);
  await page.key("Escape", "Escape", 27);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${cancel} && !${cancel}.disabled`, 15_000);
  const busy = await timed(page, null, "true");
  void busy;
  await page.sleep(2000); // the server is well into the join
  const cancelStarted = performance.now();
  await page.eval(`${cancel}.click()`);
  await page.waitFor(`${cancel}.disabled`, 30_000);
  ms = Math.round(performance.now() - cancelStarted);
  console.log(`  cancel of a cross join of 5M rows ×3: ${ms} ms`);
  assert.ok(ms < 1500, `cancel took ${ms} ms`);
  assert.match(await page.eval(`${visibleTab}.textContent`), /Query cancelled/);
  await run(page, "SELECT 42 AS answer");
  assert.equal(await page.eval(status), "1 row");
  const after = mem("after the cancel and a small query");
  assert.ok(after < 800, `${after} MB at the end`);
  void statusbar;
}

/** Runs `sql` in the visible editor; the milliseconds until the first rows are on screen. */
async function queryTime(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-12))})`);
  await page.key("Escape", "Escape", 27);
  await clearStatus(page);
  const started = performance.now();
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${firstRows}`, 120_000);
  return Math.round(performance.now() - started);
}
