// Journey (docs/COLLAUDO.md 5.5): a developer on two SQLite files. A script saved to a file
// @includes a second file (relative to the first) and runs from it; the schema check finds a
// foreign key without an index and one fix is applied; Migration status reads a Flyway
// folder; the schema is compared with a copy and the sync script is shown for review (not
// run); fake data fills two tables with valid references; after a restart the script's tab
// is back with its file; Ctrl+T then typing writes in the new editor, Escape then Tab leaves
// it. Run in English, in Italian and by keyboard.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { focusVisible, keys, setLanguage } from "../journey.mjs";
import { restart } from "../qa-jrn.mjs";
import { ui, status, visibleTab } from "../journey-work.mjs";
import { answerSave, sqliteBlock } from "../qa-xfer.mjs";

const NAME = "S jrn dev";
const COPY = "S jrn dev copy";
const SCRIPT = "@set t = s_jrn_orders\nCREATE TABLE IF NOT EXISTS s_jrn_customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n@include schema.sql\nSELECT count(*) AS n FROM ${t};";
const editorText = `[...${visibleTab}.querySelectorAll('.cm-content .cm-line')].map(l => l.textContent).join('\\n')`;
const FLYWAY = {
  "V1__create_customers.sql": "CREATE TABLE s_jrn_customers (id INTEGER PRIMARY KEY);\n",
  "V2__create_orders.sql": "CREATE TABLE s_jrn_orders (id INTEGER PRIMARY KEY); -- edited after it ran\n",
  "V3__add_index.sql": "CREATE INDEX s_jrn_orders_total ON s_jrn_orders (id);\n",
};

/** Flyway's checksum: the CRC32 of the lines without their terminators, as a signed number. */
function flywayChecksum(text) {
  let crc = 0xffffffff;
  for (const byte of Buffer.from(text.replace(/[\r\n]/g, ""), "utf8")) {
    crc ^= byte;
    for (let k = 0; k < 8; k++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (crc ^ 0xffffffff) | 0;
}

export function setup({ dir }, mode) {
  const project = join(dir, "project", "db", "migration");
  mkdirSync(project, { recursive: true });
  for (const [name, text] of Object.entries(FLYWAY)) writeFileSync(join(project, name), text);
  const db = new DatabaseSync(join(dir, "dev.db"));
  db.exec(`CREATE TABLE flyway_schema_history (installed_rank INTEGER PRIMARY KEY, version TEXT, description TEXT NOT NULL, type TEXT NOT NULL,
    script TEXT NOT NULL, checksum INTEGER, installed_by TEXT NOT NULL, installed_on TEXT NOT NULL, execution_time INTEGER NOT NULL, success INTEGER NOT NULL);`);
  const insert = db.prepare("INSERT INTO flyway_schema_history VALUES (?, ?, ?, 'SQL', ?, ?, 'dev', '2026-01-01 10:00:00', 5, 1)");
  insert.run(1, "1", "create customers", "V1__create_customers.sql", flywayChecksum(FLYWAY["V1__create_customers.sql"]));
  // V2 ran with another text than the file has now.
  insert.run(2, "2", "create orders", "V2__create_orders.sql", flywayChecksum("CREATE TABLE s_jrn_orders (id INTEGER PRIMARY KEY);"));
  db.close();
  const copy = new DatabaseSync(join(dir, "copy.db"));
  copy.exec("CREATE TABLE s_jrn_customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);");
  copy.close();
  writeFileSync(join(dir, "schema.sql"), "CREATE TABLE ${t} (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES s_jrn_customers(id), total REAL);\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${sqliteBlock(NAME, join(dir, "dev.db"), "00000000-0000-4000-8000-0000000a9302")}${sqliteBlock(COPY, join(dir, "copy.db"), "00000000-0000-4000-8000-0000000a9303")}`,
  );
  setLanguage(dir, mode);
}

const rowsOf = (file, sql) => {
  const db = new DatabaseSync(file);
  try {
    return db.prepare(sql).all().map((r) => ({ ...r }));
  } finally {
    db.close();
  }
};

/** Runs the visible tab's whole text as a script (Alt+X) and waits for the end. */
async function runScript(page, u) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  if (u.kb) await u.tabTo(`${visibleTab}.querySelector('.cm-content')`);
  else await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("x", "KeyX", 88, 1);
  const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(u.t("Cancel"))})`;
  await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes(${JSON.stringify(u.t("Executing…"))})`, 30_000);
}

/** A dialog of the schema tools, open on the tree row `chain` through `item`. */
async function openDialog(page, u, chain, item, modal) {
  await u.rowMenu(chain, [item].flat());
  await page.waitFor(`${modal} !== null`, 15_000);
}

/** Generates `rows` rows for `table` through its "Generate data…" dialog. */
async function generate(page, u, table, rows) {
  const t = u.t;
  const dialog = `document.querySelector('.modal.generate-data')`;
  await openDialog(page, u, [NAME, "main", t("Tables"), table], [u.t("Data"), t("Generate data…")], dialog);
  await page.waitFor(`${dialog}.querySelectorAll('select').length > 0`, 20_000);
  await u.italian("Generate data dialog", ["id", "name", "customer_id", "total"]);
  await u.type(`${dialog}.querySelector('input[type=number]')`, String(rows));
  await page.waitFor(`${dialog}.querySelector('input[type=number]').value === ${JSON.stringify(String(rows))}`);
  const button = (text) => `[...${dialog}.querySelectorAll('.actions button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
  await u.tap(button(t("Generate")));
  await page.waitFor(`${button(t("Close"))} !== undefined || ${dialog}.querySelector('.notice.error') !== null`, 120_000);
  assert.equal(await page.eval(`${dialog}.querySelector('.notice.error')?.textContent ?? null`), null);
  await u.tap(button(t("Close")));
  await page.waitFor(`${dialog} === null`);
}

export async function run(page, ctx, mode) {
  const u = ui(page, mode);
  const t = u.t;
  const dev = join(ctx.dir, "dev.db");
  const script = join(ctx.dir, "dev.sql");
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  await u.italian("main screen");

  // The script, typed in an editor and saved to a file.
  await u.openRow([NAME]);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await u.type(`${visibleTab}.querySelector('.cm-content')`, SCRIPT);
  await page.waitFor(`${editorText} === ${JSON.stringify(SCRIPT)}`);
  await keys.escape(page);
  await answerSave(page, script);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('dev.sql')`, 10_000);
  assert.equal(readFileSync(script, "utf8"), SCRIPT);

  // Run from its file: the @include creates the second table.
  await runScript(page, u);
  assert.deepEqual(rowsOf(dev, "SELECT name FROM sqlite_master WHERE name LIKE 's_jrn_%' AND type = 'table' ORDER BY name").map((r) => r.name), ["s_jrn_customers", "s_jrn_orders"]);
  const resultTabs = await page.eval(`[...${visibleTab}.querySelectorAll('.result-tab')].map(x => x.textContent.replace(/×$/, ''))`);
  assert.match(resultTabs.at(-1), /^\d+\. 1 (row|riga)$/, `the last result is the count: ${resultTabs}`);
  await u.italian("script results");

  // The schema check: one fix (an index on the foreign key) opened in an editor and run.
  await u.rowMenu([NAME], [t("Refresh")]);
  const lint = `document.querySelector('.modal.schema-lint')`;
  await openDialog(page, u, [NAME, "main"], [t("Analyze"), t("Check schema…")], lint);
  await page.waitFor(`${lint}.querySelectorAll('.lint-findings li').length > 0`, 15_000);
  assert.match(await page.eval(`${lint}.querySelector('.lint-findings').textContent`), /customer_id/);
  await u.italian("schema check", ["customer_id", "s_jrn_orders", "main"]);
  const tabsBefore = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await u.tap(`${lint}.querySelector('.actions button.primary')`);
  await page.waitFor(`${lint} === null && document.querySelectorAll('.query-tab').length === ${tabsBefore + 1}`, 10_000);
  await page.waitFor(`/CREATE INDEX [\\w".]*customer_id[\\w".]* ON/i.test(${editorText})`, 10_000).catch(async (e) => {
    throw new Error(`${e.message} :: ${await page.eval(editorText)}`);
  });
  await runScript(page, u);
  assert.equal(rowsOf(dev, "SELECT count(*) AS n FROM sqlite_master WHERE type = 'index' AND tbl_name = 's_jrn_orders' AND sql LIKE '%customer_id%'")[0].n, 1);
  await openDialog(page, u, [NAME, "main"], [t("Analyze"), t("Check schema…")], lint);
  await page.waitFor(`${lint}.textContent.includes(${JSON.stringify(t("No problems found."))})`, 15_000);
  await u.dismiss(lint, t("Close"));

  // Migration status on a Flyway folder.
  const migrations = `document.querySelector('.modal.schema-migrations')`;
  await openDialog(page, u, [NAME, "main"], [t("Analyze"), t("Migration status…")], migrations);
  await u.type(`${migrations}.querySelector('[data-field="folder"]')`, join(ctx.dir, "project"));
  await u.tap(`${migrations}.querySelector('[data-action="check"]')`);
  const status_of = (id) => `${migrations}.querySelector('section[data-tool="flyway"] tr[data-id="${id}"]')?.dataset.status`;
  await page.waitFor(`${status_of("3")} !== undefined`, 20_000);
  assert.deepEqual([await page.eval(status_of("1")), await page.eval(status_of("2")), await page.eval(status_of("3"))], ["applied", "divergent", "pending"]);
  await u.italian("Migration status", ["V1__create_customers.sql", "V2__create_orders.sql", "V3__add_index.sql"]);
  await u.dismiss(migrations, t("Close"));

  // The schema compared with the copy: the sync script is shown, not run.
  const compare = `document.querySelector('.modal.schema-compare')`;
  await openDialog(page, u, [NAME, "main"], [t("Analyze"), t("Compare with…")], compare);
  await page.waitFor(`document.querySelectorAll('.modal.schema-compare select')[1]?.value === 'main'`, 15_000);
  const copyId = await page.eval(`[...${compare}.querySelectorAll('select')[0].options].find(o => o.textContent === ${JSON.stringify(COPY)}).value`);
  await u.pick(`${compare}.querySelectorAll('select')[0]`, copyId);
  await page.waitFor(`${compare}.querySelectorAll('select')[1]?.value === 'main'`, 15_000);
  await u.italian("schema comparison", [COPY, NAME, "main"]);
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await u.tap(`[...${compare}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(t("Compare"))})`);
  await page.waitFor(`${compare} === null && document.querySelectorAll('.query-tab').length === ${tabs + 1}`, 30_000);
  await page.waitFor(`/CREATE TABLE main\\.s_jrn_orders/.test(${editorText})`, 15_000);
  assert.deepEqual(rowsOf(join(ctx.dir, "copy.db"), "SELECT name FROM sqlite_master WHERE type = 'table'").map((r) => r.name), ["s_jrn_customers"], "the sync script was shown, not run");

  // Fake data with valid references.
  await generate(page, u, "s_jrn_customers", 50);
  await generate(page, u, "s_jrn_orders", 200);
  assert.deepEqual(
    rowsOf(dev, `SELECT (SELECT count(*) FROM s_jrn_customers) AS customers, (SELECT count(*) FROM s_jrn_orders) AS orders,
      (SELECT count(*) FROM s_jrn_orders o WHERE customer_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM s_jrn_customers c WHERE c.id = o.customer_id)) AS orphans,
      (SELECT count(*) FROM s_jrn_orders WHERE customer_id IS NOT NULL) AS linked`),
    [{ customers: 50, orders: 200, orphans: 0, linked: 200 }],
  );

  // After a restart the script's tab is back with its file.
  await u.activateTab("dev.sql");
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('dev.sql')`);
  await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('dev.sql')`, 20_000);
  await page.waitFor(`${editorText} === ${JSON.stringify(SCRIPT)}`, 10_000);
  await u.italian("main screen after the restart");
  // Still its file: saving again writes it, with no question asked.
  await u.type(`${visibleTab}.querySelector('.cm-content')`, `${SCRIPT}\n-- edited`);
  await page.waitFor(`/-- edited/.test(${editorText})`);
  await keys.escape(page);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`document.querySelector('.modal') === null`);
  const deadline = Date.now() + 10_000;
  while (!readFileSync(script, "utf8").includes("-- edited") && Date.now() < deadline) await page.sleep(200);
  assert.ok(readFileSync(script, "utf8").endsWith("-- edited"), "Ctrl+S saved the script into its file");

  // The keyboard: Ctrl+T opens an editor that takes what is typed; Escape then Shift+Tab leaves
  // it for the toolbar (with nothing run yet there is no results panel after it).
  const before = await page.eval(`document.querySelectorAll('.tab').length`);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`document.querySelectorAll('.tab').length === ${before + 1} && ${visibleTab}?.querySelector('.cm-content') != null`, 10_000);
  await page.waitFor(`document.activeElement === ${visibleTab}.querySelector('.cm-content')`, 5_000);
  await page.type("select 1");
  await page.waitFor(`${editorText} === 'select 1'`, 5_000);
  await keys.escape(page);
  await page.eval(`void (window.__focusBefore = document.activeElement)`);
  await keys.tab(page, 8);
  await page.waitFor(`document.activeElement !== window.__focusBefore && document.activeElement !== document.body && !document.activeElement.closest('.cm-content')`, 5_000).catch(async (e) => { throw new Error(`${e.message} :: ${await page.eval(`(() => { const a = document.activeElement; return a.tagName + '.' + a.className + ' | before: ' + (window.__focusBefore?.className) + ' | text: ' + ${editorText}; })()`)}`); });
  assert.ok(await page.eval(focusVisible), "the focus is visible after leaving the editor");
}
