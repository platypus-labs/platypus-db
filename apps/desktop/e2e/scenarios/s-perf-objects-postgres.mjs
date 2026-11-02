// Group S, performance: a PostgreSQL schema with thousands of objects (s_perf_objs: 5,000 tables
// chained by foreign keys, 500 views, 200 functions; made once and reused while it is there).
// In the app, each timed and bounded, the numbers printed: expanding the schema and its
// folders in the navigator, the navigator filter, completion of a table name in the editor, the
// metadata search (a narrow one and one with thousands of hits), a table's object page, and the
// ER diagram of a subset of the tables. The memory of the app is printed after each step.
// Needs PostgreSQL (docker compose -f docker/compose.yml up -d).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { PG, lag, memoryNow, needsPort, openEditor, pgEntry, saveConnections, startFrames, stopFrames, timed, treeRow } from "../perf.mjs";
import { clearStatus, treeMenu } from "../qa-grid.mjs";
import { connectWith, openPath, psql } from "../qa-jrn.mjs";

export const skip = needsPort(PG.port, "PostgreSQL");

const SCHEMA = "s_perf_objs";
const TABLES = 5000;
const VIEWS = 500;
const FUNCTIONS = 200;
const table = (i) => `t_${String(i).padStart(5, "0")}`;
const folder = (name, n) => `[...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(name)} && r.textContent.replace(/[.,\\s]/g, '').endsWith('${name}${n}'))`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const searchTab = `[...document.querySelectorAll('.search-tab')].find(t => t.style.display !== 'none')`;
const objectTab = `[...document.querySelectorAll('.object-tab:not(.search-tab)')].find(t => t.style.display !== 'none')`;
const visibleErd = `[...document.querySelectorAll('.erd')].find(t => t.style.display !== 'none')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const options = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;
const filter = `document.querySelector('.tree-filter')`;

export function setup({ dir }) {
  saveConnections(dir, pgEntry());
  const count = (kind) => Number(psql(kind));
  const have = [
    `SELECT count(*) FROM pg_tables WHERE schemaname = '${SCHEMA}'`,
    `SELECT count(*) FROM pg_views WHERE schemaname = '${SCHEMA}'`,
    `SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = '${SCHEMA}'`,
  ].map(count);
  if (have[0] === TABLES && have[1] === VIEWS && have[2] === FUNCTIONS) return;
  console.log(`  creating schema ${SCHEMA} (${TABLES} tables, ${VIEWS} views, ${FUNCTIONS} functions), once`);
  // Each statement alone: COMMIT inside a DO block needs no surrounding transaction, and many
  // CREATE/DROP TABLE in one transaction overflow the lock table.
  psql(`DO $$ DECLARE r record; n int := 0; BEGIN
      FOR r IN SELECT tablename FROM pg_tables WHERE schemaname = '${SCHEMA}' LOOP
        EXECUTE format('DROP TABLE %I.%I CASCADE', '${SCHEMA}', r.tablename); n := n + 1; IF n % 500 = 0 THEN COMMIT; END IF;
      END LOOP; END $$`);
  psql(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
  psql(`CREATE SCHEMA ${SCHEMA}`);
  psql(`DO $$ BEGIN FOR i IN 1..${TABLES} LOOP
      EXECUTE format('CREATE TABLE ${SCHEMA}.t_%s (id int PRIMARY KEY, parent_id int %s, name text, value numeric)', lpad(i::text, 5, '0'),
        CASE WHEN i = 1 THEN '' ELSE format('REFERENCES ${SCHEMA}.t_%s(id)', lpad((i - 1)::text, 5, '0')) END);
      IF i % 500 = 0 THEN COMMIT; END IF;
    END LOOP; END $$`);
  psql(`DO $$ BEGIN FOR i IN 1..${VIEWS} LOOP
      EXECUTE format('CREATE VIEW ${SCHEMA}.v_%s AS SELECT id, name FROM ${SCHEMA}.t_%s', lpad(i::text, 5, '0'), lpad(i::text, 5, '0'));
    END LOOP; END $$`);
  psql(`DO $$ BEGIN FOR i IN 1..${FUNCTIONS} LOOP
      EXECUTE format('CREATE FUNCTION ${SCHEMA}.f_%s(a int) RETURNS int LANGUAGE sql AS ''SELECT a + %s''', lpad(i::text, 5, '0'), i);
    END LOOP; END $$`);
  psql(`ANALYZE`);
}

/** Types `text` as the editor's whole text, then `typed` after it key by key. */
async function typeAfter(page, text, typed) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.waitFor(`${editorText} === ${JSON.stringify(text)}`);
  await page.key("Escape", "Escape", 27);
  for (const ch of typed) {
    await page.type(ch);
    await page.sleep(20);
  }
  await page.waitFor(`${editorText} === ${JSON.stringify(text + typed)}`);
}

async function search(page, text) {
  await page.waitFor(`${searchTab}?.querySelector('.search-input') != null`);
  await page.eval(`${searchTab}.querySelector('.search-input').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.waitFor(`${searchTab}.querySelector('.search-input').value === ${JSON.stringify(text)}`);
  await page.eval(`${searchTab}.querySelector('.search-form button[type=submit]').click()`);
}

export default async function (page, { dir }) {
  const mem = (label) => memoryNow(dir, label);
  await connectWith(page, PG.name);
  await openPath(page, [PG.name, "platypus"], SCHEMA);
  mem("connected");

  // 1. Expanding the schema and its folders.
  await page.eval(`${treeRow(SCHEMA)}.id = 's-schema'; document.getElementById('s-schema').scrollIntoView({ block: 'start' })`);
  let ms = await timed(page, `document.querySelector('#s-schema .twisty').click()`, folder("Tables", TABLES), 120_000);
  console.log(`  expanding the schema (${TABLES} tables listed): ${ms} ms`);
  assert.ok(ms < 500, `expanding took ${ms} ms`);
  // Tables opens on its own; fold it so the other folders are near, then open each in turn.
  await page.eval(`(() => { const r = [...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === "Tables" && r.textContent.endsWith("5000")); r.querySelector('.twisty').click(); })()`);
  await page.waitFor(`${folder("Views", VIEWS)} && ${folder("Functions", FUNCTIONS)}`, 30_000);
  for (const name of ["Functions", "Views", "Tables"]) {  // bottom-up: an open folder pushes the next ones out of view
    const n = { Tables: TABLES, Views: VIEWS, Functions: FUNCTIONS }[name];
    const first = name === "Tables" ? table(1) : name === "Views" ? "v_00001" : "f_00001";
    ms = await timed(
      page,
      `(() => { const r = [...document.querySelectorAll('.tree-row')].findLast(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(name)}); r.scrollIntoView({ block: 'start' }); r.querySelector('.twisty').click(); })()`,
      `[...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent?.startsWith(${JSON.stringify(first)}))`,
      60_000,
    );
    console.log(`  expanding ${name} (${n}): ${ms} ms`);
    assert.ok(ms < 500, `expanding ${name} took ${ms} ms`);
  }
  const rendered = await page.eval(`document.querySelectorAll('.tree-row').length`);
  assert.ok(rendered < 1500, `${rendered} tree rows in the DOM: the navigator renders only what is in view`);
  mem("schema expanded");

  // 2. The navigator filter: one table among the 5,000, none, and back.
  await startFrames(page);
  await page.eval(`${filter}.focus()`);
  ms = await timed(page, null, "true");
  const started = performance.now();
  await page.type(table(4999));
  await page.waitFor(`document.querySelectorAll('.tree-row').length < 12 && [...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(table(4999))})`, 20_000);
  ms = Math.round(performance.now() - started);
  console.log(`  tree filter "${table(4999)}": ${ms} ms`);
  assert.ok(ms < 500, `filter took ${ms} ms`);
  await page.type("x");
  await page.waitFor(`![...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(table(4999))})`, 10_000);
  await page.key("Escape", "Escape", 27);
  ms = await timed(page, null, `${filter}.value === '' && [...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(table(1))})`, 20_000);
  console.log(`  filter cleared, tree back: ${ms} ms`);
  const typing = await stopFrames(page);
  console.log(`  frames while filtering: ${typing.frames}, longest ${typing.max} ms`);
  assert.ok(typing.max < 300, `a frame took ${typing.max} ms`);

  // 3. Completion of a table name in the editor, qualified with the schema.
  await openEditor(page, PG.name);
  const completeStarted = performance.now();
  await typeAfter(page, `SELECT * FROM ${SCHEMA}.`, "t_0499");
  await page.waitFor(`${options}.some(o => o.startsWith(${JSON.stringify(table(4999))}))`, 20_000);
  ms = Math.round(performance.now() - completeStarted);
  const listed = await page.eval(options);
  console.log(`  table name completion: ${ms} ms (${listed.length} options)`);
  assert.ok(ms < 1500, `completion took ${ms} ms`);
  assert.ok(!listed.some((o) => o.startsWith(table(1))), "the list keeps to the typed prefix");
  await page.sleep(200); // CodeMirror ignores keys for 75 ms after the list opens
  await page.key("Escape", "Escape", 27);
  mem("completion");

  // 4. The global (metadata) search: a narrow one, and one with thousands of hits.
  await treeMenu(page, PG.name, "Search objects…");
  await page.waitFor(`${searchTab}?.querySelector('.search-input') != null`, 15_000);
  const hits = `[...(${searchTab}?.querySelectorAll('.search-results tbody tr') ?? [])].map(r => r.textContent.trim())`;
  await search(page, "t_0499");
  ms = await timed(page, null, `${hits}.some(t => t.startsWith(${JSON.stringify(table(4999))}))`, 60_000);
  console.log(`  search "t_0499": ${ms} ms, ${(await page.eval(hits)).length} hits`);
  assert.ok(ms < 500, `search took ${ms} ms`);
  await search(page, "parent_id");
  ms = await timed(page, null, `${hits}.length > 100`, 60_000);
  const many = (await page.eval(hits)).length;
  console.log(`  search "parent_id" (a column of every table): ${ms} ms, ${many} hits shown`);
  assert.ok(ms < 1000, `the big search took ${ms} ms`);
  const idle = await lag(page);
  console.log(`  page answers in ${Math.round(idle)} ms after it`);
  assert.ok(idle < 200, `the page was busy for ${Math.round(idle)} ms`);
  mem("search");

  // 5. One table's object page, from the filtered navigator.
  await page.eval(`${filter}.focus()`);
  await page.type(table(2500));
  await page.waitFor(`document.querySelectorAll('.tree-row').length < 12 && [...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(table(2500))})`, 20_000);
  const opened = performance.now();
  await treeMenu(page, table(2500), "Open details");
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === ${JSON.stringify(table(2500))} && /parent_id/.test(${objectTab}.textContent)`, 30_000);
  ms = Math.round(performance.now() - opened);
  console.log(`  object page of ${table(2500)}: ${ms} ms (includes the menu)`);
  assert.ok(ms < 1000, `the object page took ${ms} ms`);
  await page.eval(`${filter}.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${filter}.value === ''`);

  // 6. The ER diagram of a subset: 100 tables, with the 99 foreign keys that join them.
  await clearStatus(page);
  await treeMenu(page, SCHEMA, "ER diagram");
  const picker = `document.querySelector('.modal.erd-tables')`;
  await page.waitFor(`${picker} !== null`, 60_000);
  // Every table starts chosen: clear them all, then choose just the filtered 100.
  await page.eval(`[...${picker}.querySelectorAll('button')].find(b => b.textContent === 'Clear shown').click()`);
  await page.waitFor(`/^(0|No) /.test(${picker}.querySelector('.muted')?.textContent ?? '')`, 10_000);
  await page.eval(`${picker}.querySelector('input[aria-label="Filter"]').focus()`);
  await page.type("t_020");
  await page.waitFor(`${picker}.querySelectorAll('.erd-table-list li').length === 100`, 10_000);
  await page.eval(`[...${picker}.querySelectorAll('button')].find(b => b.textContent === 'Select shown').click()`);
  await page.waitFor(`/^100 tables? chosen/.test(${picker}.querySelector('.muted')?.textContent ?? '')`, 10_000);
  ms = await timed(page, `[...${picker}.querySelectorAll('button')].find(b => b.textContent === 'Show').click()`, `${visibleErd}?.querySelectorAll('.erd-table').length === 100`, 120_000);
  console.log(`  ER diagram of 100 tables: ${ms} ms`);
  assert.ok(ms < 3000, `the diagram took ${ms} ms`);
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('path[marker-start]').length`), 99, "the 99 foreign keys are drawn");
  const final = mem("ER diagram");
  assert.ok(final < 900, `${final} MB at the end`);
}
