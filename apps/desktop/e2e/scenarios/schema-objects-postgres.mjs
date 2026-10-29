// Creating a function, a trigger, a sequence and a materialized view from the forms of the
// navigator, against the dev PostgreSQL from docker/compose.yml. Skipped when it is not
// running. What the database holds is read with psql in the container.
import assert from "node:assert/strict";
import { ALT, CTRL } from "../cdp.mjs";
import { psql } from "../qa3-schema.mjs";
import {
  choose,
  create,
  createMenu,
  editorText,
  errorIs,
  fill,
  form,
  openCreate,
  openTableTrigger,
  options,
  preview,
  previewHas,
  rightClick,
  reveal,
  row,
  tick,
} from "../schema-objects.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;

/** Replaces the editor's text and runs it as a script. */
async function run(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-20))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("x", "KeyX", 88, ALT);
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 15_000);
}

const CLEAN = `DROP MATERIALIZED VIEW IF EXISTS demo.e2e_mv;
DROP FUNCTION IF EXISTS demo.e2e_add(integer, integer);
DROP TABLE IF EXISTS demo.e2e_objects CASCADE;
DROP FUNCTION IF EXISTS demo.e2e_trg_fn() CASCADE;
DROP SEQUENCE IF EXISTS demo.e2e_seq;
DROP SEQUENCE IF EXISTS demo.e2e_seq_ed;`;

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);

  // A table to put triggers on, made in the editor of the connection.
  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await run(page, `${CLEAN}\nCREATE TABLE demo.e2e_objects (id int, note text);`);
  try {
    await forms(page);
  } finally {
    psql(CLEAN);
  }
}

async function forms(page) {
  // The new table shows once the schema is read again.
  if (await page.eval(`${row("demo")}.getAttribute('aria-expanded') === 'true'`)) {
    await rightClick(page, "demo");
    await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Refresh').click()`);
  } else {
    await page.eval(`${row("demo")}.querySelector(".twisty").click()`);
  }
  await page.waitFor(`${row("e2e_objects")} !== undefined`, 15_000);

  // PostgreSQL has all four.
  assert.deepEqual(await createMenu(page, "demo"), ["View…", "Function…", "Trigger…", "Sequence…"]);
  await page.eval(`[...document.querySelectorAll('.context-menu.submenu button')].find(b => b.querySelector('span').textContent === 'Function…').click()`);
  await page.waitFor(`${form} !== null`);

  // A function with two parameters; an empty parameter is refused.
  await fill(page, "name", "e2e_add");
  await page.eval(`${form}.querySelector('[name=add-param]').click()`);
  await errorIs(page, "Every parameter needs a name.");
  await page.eval(`${form}.querySelector('[name=add-param]').click()`);
  await fill(page, "param-name-0", "a");
  await fill(page, "param-type-0", "integer");
  await fill(page, "param-name-1", "b");
  await fill(page, "param-type-1", "integer");
  await fill(page, "returns", "integer");
  await fill(page, "body", "RETURN a + b;");
  await previewHas(page, "CREATE FUNCTION demo.e2e_add(a integer, b integer) RETURNS integer LANGUAGE plpgsql AS $body$");
  await create(page);
  await reveal(page, "Functions", "e2e_add");
  assert.equal(psql("SELECT demo.e2e_add(2, 3)"), "5");

  // A trigger from the table's menu: a function and the trigger on it, two statements.
  await openTableTrigger(page, "e2e_objects");
  await page.waitFor(`${form}.querySelector('[name=table]').value === 'e2e_objects'`, 15_000);
  assert.deepEqual(await options(page, "timing"), ["BEFORE", "AFTER", "INSTEAD OF"]);
  await choose(page, "timing", "before");
  await fill(page, "name", "e2e_trg");
  await tick(page, "event-update", true);
  await fill(page, "when", "NEW.note IS NULL");
  await fill(page, "body", "NEW.note := 'filled';");
  await previewHas(page, "CREATE OR REPLACE FUNCTION demo.e2e_trg_fn() RETURNS trigger LANGUAGE plpgsql");
  await previewHas(page, "CREATE TRIGGER e2e_trg BEFORE INSERT OR UPDATE ON demo.e2e_objects FOR EACH ROW WHEN (NEW.note IS NULL) EXECUTE FUNCTION demo.e2e_trg_fn()");
  assert.ok((await page.eval(preview)).includes("RETURN NEW;"));
  // No event, no trigger.
  await tick(page, "event-insert", false);
  await tick(page, "event-update", false);
  await errorIs(page, "A trigger needs at least one event.");
  await tick(page, "event-insert", true);
  await tick(page, "event-update", true);
  await previewHas(page, "BEFORE INSERT OR UPDATE");
  await create(page);
  psql("INSERT INTO demo.e2e_objects (id) VALUES (1)");
  assert.equal(psql("SELECT note FROM demo.e2e_objects WHERE id = 1"), "filled");

  // A sequence; bad numbers are told apart from a zero increment.
  await openCreate(page, "demo", "Sequence…");
  await fill(page, "name", "e2e_seq");
  await fill(page, "start", "x");
  await errorIs(page, "Sequence options must be whole numbers.");
  await fill(page, "start", "100");
  await fill(page, "increment", "0");
  await errorIs(page, "A sequence cannot increment by 0.");
  await fill(page, "increment", "10");
  await fill(page, "max", "5000");
  await tick(page, "cycle", true);
  await previewHas(page, "CREATE SEQUENCE demo.e2e_seq START WITH 100 INCREMENT BY 10 MAXVALUE 5000 CYCLE;");
  await create(page);
  await reveal(page, "Sequences", "e2e_seq");
  assert.equal(psql("SELECT nextval('demo.e2e_seq')"), "100");

  // A materialized view, then replaced: PostgreSQL drops it first.
  await openCreate(page, "demo", "View…");
  await fill(page, "name", "e2e_mv");
  await fill(page, "query", "SELECT id FROM demo.e2e_objects");
  await tick(page, "materialized", true);
  await previewHas(page, "CREATE MATERIALIZED VIEW demo.e2e_mv AS SELECT id FROM demo.e2e_objects;");
  await create(page);
  await reveal(page, "Materialized views", "e2e_mv");
  assert.equal(psql("SELECT count(*) FROM demo.e2e_mv"), "1");
  psql("INSERT INTO demo.e2e_objects (id, note) VALUES (2, 'x')");
  await openCreate(page, "demo", "View…");
  await fill(page, "name", "e2e_mv");
  await fill(page, "query", "SELECT id FROM demo.e2e_objects");
  await tick(page, "materialized", true);
  await tick(page, "replace", true);
  await previewHas(page, "DROP MATERIALIZED VIEW IF EXISTS demo.e2e_mv;");
  await create(page);
  assert.equal(psql("SELECT count(*) FROM demo.e2e_mv"), "2");

  // Open in editor shows the same script and runs nothing.
  await openCreate(page, "demo", "Sequence…");
  await fill(page, "name", "e2e_seq_ed");
  await previewHas(page, "CREATE SEQUENCE demo.e2e_seq_ed;");
  await page.eval(`${form}.querySelector('button[name=open-in-editor]').click()`);
  await page.waitFor(`${form} === null`);
  await page.waitFor(`(${editorText}).includes('CREATE SEQUENCE demo.e2e_seq_ed')`, 15_000);
  assert.equal(psql("SELECT count(*) FROM pg_class WHERE relname = 'e2e_seq_ed'"), "0");
}
