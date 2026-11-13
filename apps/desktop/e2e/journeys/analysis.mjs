// Journey (docs/dev/COLLAUDO.md 5.4): an analyst on a SQLite file. A result of 100,000 rows loaded
// in full; SQL on the result; the statistics of a column; the chart exported to PNG; the
// result exported to Excel and to Parquet (files checked); an .xlsx and the .parquet imported
// into new tables with the right types; the data changes, the statement runs again and
// "Compare with previous run" finds the changed rows; the result is saved; after a restart
// the analysis tabs and the saved result are back. Run in English, in Italian and by keyboard.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { keys, press, setLanguage } from "../journey.mjs";
import { restart } from "../journey-helpers.mjs";
import { ui, status, visibleTab } from "../journey-work.mjs";
import { chart, compareButton, diffTab, field, fieldError, fieldText, mark, profileOf } from "../analysis-helpers.mjs";
import { parquetInfo } from "../parquet.mjs";
import { answerSave, sqliteBlock, zipEntry } from "../transfer-helpers.mjs";
import { writeXlsx } from "../xlsx.mjs";

const NAME = "S jrn analysis";
const ID = "00000000-0000-4000-8000-0000000a9301";
const SQL = "select * from s_jrn_sales order by id";
const digits = (s) => Number(String(s).replace(/\D/g, ""));
const open = (file) => new DatabaseSync(file);
const rowsOf = (file, sql) => {
  const db = open(file);
  try {
    return db.prepare(sql).all().map((r) => ({ ...r }));
  } finally {
    db.close();
  }
};

export function setup({ dir }, mode) {
  const file = join(dir, "sales.db");
  const db = open(file);
  db.exec(`CREATE TABLE s_jrn_sales (id INTEGER PRIMARY KEY, region TEXT, grp INTEGER, amount REAL);
    WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 100000)
      INSERT INTO s_jrn_sales SELECT n, 'r' || (n % 7), n % 100, round(((n * 37) % 1000) / 3.0, 2) FROM g;`);
  db.close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, file, ID)}`);
  writeXlsx(join(dir, "people.xlsx"), {
    People: [
      ["id", "name", "score", "active"],
      [1, "Ada", 9.5, true],
      [2, "Bo", 7.25, false],
      [3, "Cy", null, true],
    ],
  });
  setLanguage(dir, mode);
}

/** Picks the Export dialog's format and exports the visible tab's loaded rows to `path`. */
async function exportTo(page, u, format, path) {
  const t = u.t;
  await u.tap(`[...${visibleTab}.querySelectorAll('button.link')].find(b => b.textContent === ${JSON.stringify(t("Export…"))})`);
  const dialog = `document.querySelector('.modal.export-result')`;
  await page.waitFor(`${dialog} !== null`);
  await u.italian("Export dialog");
  await u.pick(`${dialog}.querySelector('select')`, format);
  await u.check(`${dialog}.querySelectorAll('fieldset.choices input')[1]`);
  await page.waitFor(`${dialog}.querySelectorAll('fieldset.choices input')[1].checked`);
  const saved = `[...${visibleTab}.querySelectorAll('span')].find(s => /^(Exported|Esportat|Export failed|Esportazione)/.test(s.textContent))`;
  await answerSave(page, path);
  await u.tap(`${dialog}.querySelector('button[type=submit]')`);
  const deadline = Date.now() + 120_000;
  while (!existsSync(path) && Date.now() < deadline) await page.sleep(300);
  await page.waitFor(`${saved} !== undefined`, 120_000);
  const text = await page.eval(`${saved}.textContent`);
  assert.doesNotMatch(text, /failed|fallit/i, text);
  // Dismissed (a button of the status bar), so the next export's outcome is a new text.
  await u.tap(`${saved}.querySelector('button')`);
  await page.waitFor(`${saved} === undefined`, 5_000);
  return text;
}

/** Imports `file` into a new table `table` of the connection's main schema. */
async function importInto(page, u, file, table) {
  const t = u.t;
  await u.rowMenu([NAME, "main"], [t("Data"), t("Import data into a new table…")]);
  const dialog = `document.querySelector('.modal.import-data')`;
  await page.waitFor(`${dialog} !== null`, 15_000);
  await page.waitFor(`!(${dialog}.querySelector('button')?.disabled) && [...${dialog}.querySelectorAll('button')].some(b => b.textContent.trim() === ${JSON.stringify(t("Choose file…"))} && !b.disabled)`, 20_000);
  await u.type(`${dialog}.querySelector('input')`, file);
  await page.waitFor(`${dialog}.querySelector('input').value === ${JSON.stringify(file)}`);
  await (await import("../journey.mjs")).keys.enter(page);
  await page.waitFor(`${dialog}.querySelector('.import-preview') !== null || ${dialog}.querySelector('.notice.error') !== null`, 60_000);
  assert.equal(await page.eval(`${dialog}.querySelector('.notice.error')?.textContent ?? null`), null);
  await u.italian("Import dialog", ["id", "name", "score", "active", "region", "grp", "amount"]);
  await u.type(`${dialog}.querySelector('label.field:not(.grow) input')`, table);
  await page.waitFor(`${dialog}.querySelector('label.field:not(.grow) input').value === ${JSON.stringify(table)}`);
  const importButton = `[...${dialog}.querySelectorAll('.actions button')].find(b => b.textContent.trim() === ${JSON.stringify(t("Import"))})`;
  await u.tap(importButton);
  const done = `${dialog}.querySelector('.actions button')?.textContent.trim() === ${JSON.stringify(t("Close"))} || ${dialog}.querySelector('.notice.error') !== null`;
  await page.waitFor(done, 180_000);
  const out = await page.eval(`[...${dialog}.querySelectorAll('.notice')].map(n => n.textContent).join(' | ')`);
  await u.tap(`[...${dialog}.querySelectorAll('.actions button')].find(b => b.textContent.trim() === ${JSON.stringify(t("Close"))})`);
  await page.waitFor(`${dialog} === null`);
  return out;
}

export async function run(page, ctx, mode) {
  const u = ui(page, mode);
  const t = u.t;
  const file = join(ctx.dir, "sales.db");
  const png = join(ctx.dir, "chart.png");
  const xlsx = join(ctx.dir, "sales.xlsx");
  const parquet = join(ctx.dir, "sales.parquet");

  // A result of 100,000 rows, loaded in full.
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  await u.italian("main screen");
  await u.openRow([NAME]);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await u.runSql(SQL);
  await u.tap(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === ${JSON.stringify(t("Fetch all"))})`);
  await page.waitFor(`/^[\\d.,]+ (rows?|righ[ae])$/.test(${status}.trim()) && ${status}.replace(/\\D/g, '') === '100000'`, 120_000);
  await u.italian("result of 100,000 rows");

  // SQL on the loaded rows.
  const expected = rowsOf(file, "SELECT count(*) AS n FROM s_jrn_sales WHERE grp = 5 AND region = 'r3'")[0].n;
  const inField = async (sql) => {
    await u.type(`${field}`, sql);
    await page.waitFor(`${fieldText} === ${JSON.stringify(sql)}`);
    if (await page.eval(`document.querySelector('.cm-tooltip-autocomplete') !== null`)) await page.key("Escape", "Escape", 27);
    await page.waitFor(`document.querySelector('.cm-tooltip-autocomplete') === null`);
    const executing = JSON.stringify(t("Executing…"));
    await page.eval(`(() => {
      window.__fieldBusy = false;
      window.__fieldObserver?.disconnect();
      const bar = ${visibleTab}.querySelector('.result-sql');
      window.__fieldObserver = new MutationObserver(() => { if (bar.textContent.includes(${executing})) window.__fieldBusy = true; });
      window.__fieldObserver.observe(bar, { subtree: true, childList: true, characterData: true });
    })()`);
    await page.key("Enter", "Enter", 13);
    await page.waitFor(`window.__fieldBusy === true`, 5_000);
    await page.waitFor(`!(${visibleTab}.querySelector('.result-sql')?.textContent.includes(${executing}))`, 60_000);
    assert.equal(await page.eval(fieldError), "", "SQL on the result failed");
    return page.eval(mark);
  };
  assert.equal(digits(await inField("WHERE grp = 5 AND region = 'r3'")), expected);
  assert.equal(digits(await inField("SELECT region, count(*) AS n, sum(amount) AS total FROM result GROUP BY region ORDER BY region")), 7);
  await u.italian("SQL on the result");

  // The statistics of a column: the box under its header (mouse), or the table's profile tab.
  if (!u.kb) {
    const profile = await profileOf(page, "n");
    assert.equal(profile.fields[t("Distinct")], "2");
  }

  // The chart of the grouped rows, exported to PNG.
  const toggle = (label) => `[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === ${JSON.stringify(t(label))})`;
  await u.tap(toggle("Chart"));
  await page.waitFor(`${chart}?.querySelectorAll('rect.chart-bar').length === 14`, 15_000);
  await u.italian("chart");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(png)})`);
  await u.tap(`[...${visibleTab}.querySelectorAll('.chart-export button')].find(b => b.textContent === ${JSON.stringify(t("PNG image…"))})`);
  const deadline = Date.now() + 15_000;
  while (!existsSync(png) && Date.now() < deadline) await page.sleep(200);
  const bytes = readFileSync(png);
  assert.deepEqual([...bytes.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], "a PNG signature");
  assert.ok(bytes.length > 2_000, "the PNG has a picture in it");
  await u.tap(toggle("Grid"));

  // Back to the full result (Escape in the SQL field, or its × button).
  if (u.kb) {
    await u.tabTo(field);
    await page.key("Escape", "Escape", 27);
  } else await page.eval(`${visibleTab}.querySelector('.result-sql-clear').click()`);
  await page.waitFor(`${status}.replace(/\\D/g, '') === '100000'`, 15_000);

  // Excel and Parquet files of the whole result.
  assert.match(await exportTo(page, u, "xlsx", xlsx), /100[.,]000/);
  const sheet = zipEntry(xlsx, "xl/worksheets/sheet1.xml");
  assert.ok(sheet, "the workbook has a sheet");
  assert.equal(sheet.match(/<row /g).length, 100_001, "a header and 100,000 rows");
  assert.ok(zipEntry(xlsx, "[Content_Types].xml") && zipEntry(xlsx, "xl/workbook.xml"), "a workbook, not just a sheet");
  assert.match(await exportTo(page, u, "parquet", parquet), /100[.,]000/);
  const info = parquetInfo(parquet);
  assert.equal(info.rows, 100_000);
  assert.deepEqual(info.columns.map((c) => c.name), ["id", "region", "grp", "amount"]);

  // An .xlsx and a .parquet imported into new tables, with the right types.
  assert.match(await importInto(page, u, join(ctx.dir, "people.xlsx"), "s_jrn_people"), /3/);
  assert.deepEqual(rowsOf(file, "SELECT id, name, score FROM s_jrn_people ORDER BY id"), [
    { id: 1, name: "Ada", score: 9.5 },
    { id: 2, name: "Bo", score: 7.25 },
    { id: 3, name: "Cy", score: null },
  ]);
  assert.deepEqual(rowsOf(file, "SELECT typeof(id) AS i, typeof(name) AS n, typeof(score) AS s FROM s_jrn_people WHERE id = 1"), [{ i: "integer", n: "text", s: "real" }]);
  assert.match(await importInto(page, u, parquet, "s_jrn_from_parquet"), /100[.,]?000/);
  assert.deepEqual(rowsOf(file, "SELECT count(*) AS n, sum(typeof(id) = 'integer') AS ints, sum(typeof(region) = 'text') AS texts, sum(typeof(amount) = 'real') AS reals FROM s_jrn_from_parquet"), [
    { n: 100_000, ints: 100_000, texts: 100_000, reals: 100_000 },
  ]);
  assert.deepEqual(rowsOf(file, "SELECT sum(amount) AS a FROM s_jrn_from_parquet"), rowsOf(file, "SELECT sum(amount) AS a FROM s_jrn_sales"));

  // The data changes; the same statement again; the comparison finds the changed rows.
  const db = open(file);
  db.exec("UPDATE s_jrn_sales SET amount = amount + 1 WHERE id <= 10; DELETE FROM s_jrn_sales WHERE id = 99999");
  db.close();
  await u.runSql(SQL);
  await u.tap(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === ${JSON.stringify(t("Fetch all"))})`);
  await page.waitFor(`${status}.replace(/\\D/g, '') === '99999'`, 120_000);
  await page.waitFor(`${compareButton.replace("'Compare with previous run'", JSON.stringify(t("Compare with previous run")))}?.disabled === false`, 15_000);
  await u.tap(compareButton.replace("'Compare with previous run'", JSON.stringify(t("Compare with previous run"))));
  await page.waitFor(`${diffTab}?.querySelector('.diff-counts') != null`, 60_000);
  assert.equal(await page.eval(`${diffTab}.querySelector('.diff-counts').textContent`), "+0 −1 ~10");
  await u.italian("comparison");

  // The result saved under a name: the grid menu, or the command palette by keyboard.
  await u.activateTab(NAME);
  const dialogInput = `document.querySelector('.modal.name-dialog input')`;
  if (u.kb) {
    // Shift+F10 on the grid opens its menu.
    await u.tabTo(`${visibleTab}.querySelector('.grid-scroll')`);
    await press(page, "F10", "F10", 121, 8);
  } else {
    await page.eval(`${visibleTab}.querySelector('.grid-scroll').id = 'jrn-grid'`);
    const r = await page.rect("#jrn-grid");
    await page.eval(`document.getElementById('jrn-grid').removeAttribute('id')`);
    await page.click(r.x + 40, r.y + 40, { button: "right" });
  }
  await u.chooseMenu([t("Save result…")]);
  await page.waitFor(`${dialogInput} != null`, 30_000);
  await u.italian("save result dialog");
  await u.type(dialogInput, "S jrn weekly sales");
  await u.tap(`document.querySelector('.modal.name-dialog button[type=submit]')`);
  const savedRow = `[...document.querySelectorAll('.saved-result-row')].find(r => r.dataset.name === "S jrn weekly sales")`;
  await page.waitFor(`${savedRow} !== undefined`, 30_000);
  const kept = readdirSync(join(ctx.dir, "saved-results")).filter((f) => f.endsWith(".parquet"));
  assert.equal(kept.length, 1);
  assert.equal(parquetInfo(join(ctx.dir, "saved-results", kept[0])).rows, 99_999);

  // The table's profile, an analysis tab of its own.
  await u.rowMenu([NAME, "main", t("Tables"), "s_jrn_sales"], [t("Profile data…")]);
  await page.waitFor(`document.querySelector('.column-profile-head') !== null`, 60_000);
  await u.italian("table profile");
  const profileTab = `[...document.querySelectorAll('.tab')].find(x => x.textContent.includes("s_jrn_sales") && x.textContent !== ${JSON.stringify(NAME)})`;
  await page.waitFor(`${profileTab} !== undefined`);
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('s_jrn_sales')`);

  // After a restart: the analysis tabs and the saved result are back.
  await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`${profileTab} !== undefined`, 20_000);
  await page.waitFor(`[...document.querySelectorAll('.tab')].some(x => x.textContent.startsWith(${JSON.stringify(NAME)}))`, 20_000);
  await page.waitFor(`${savedRow} !== undefined`, 20_000);
  await u.italian("main screen after the restart");
  // The saved result opens with its rows, from the file.
  if (u.kb) {
    await u.tabTo(savedRow);
    await keys.enter(page);
  } else await page.eval(`${savedRow}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`[...document.querySelectorAll('.result-source')].some(n => /\\b99[.,]999\\b/.test(n.textContent))`, 60_000);
}
