// Complete user journey 1, the first start (docs/dev/COLLAUDO.md 5.1): an empty profile opens on the
// home screen; a PostgreSQL connection (docker, port 55432) is made with a wrong password first
// (a readable error) and then the right one, saved; a SQLite one; a table opened, a cell edited
// and saved; a query in a new editor, exported to CSV; the app restarted (the connections connect
// without asking, the tabs are back); the language changed in the Settings and the app restarted
// again (the interface is in the chosen language). Run in English, in Italian and keyboard only.
import { pressF2 } from "../grid-helpers.mjs";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { savedConnections } from "../import-helpers.mjs";
import {
  restartApp, fmt, journey, keys, openSettings, closeSettings, setSettingSelect, pickDriver, pressButton, screenIsItalian, selectEl, setField,
  tabTo, testConnection, treeOpen, until, visibleTab, waitRows,
} from "../journey-day.mjs";
import { setLanguage } from "../journey.mjs";
import { psql } from "../journey-helpers.mjs";
import { answerSave } from "../transfer-helpers.mjs";

const PG = "Journey PG";
const LITE = "Journey SQLite";
const TABLE = "s_jrn_first";
const SQL = `select name, price from ${TABLE} order by id`;
const tabTitles = `[...document.querySelectorAll('.tab')].map(t => t.textContent.replace(/×$/, ''))`;
const isPg = (name) => `SELECT string_agg(name, ',' ORDER BY id) FROM ${name}`;

export function setup({ dir }, mode) {
  setLanguage(dir, mode);
  const db = new DatabaseSync(join(dir, "s_jrn_first.db"));
  db.exec("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes VALUES (1, 'uno'), (2, 'due');");
  db.close();
  psql(
    `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id serial PRIMARY KEY, name text, price numeric(12,2)); ` +
      `INSERT INTO ${TABLE} (name, price) VALUES ('alpha', 1.50), ('beta', 2.25), ('gamma', NULL);`,
  );
}

/** Opens the picker: the welcome page's button on the first start, the navigator's after. */
export async function newConnection(j, first) {
  const { page } = j;
  if (first) {
    if (j.kb) await tabTo(j, `el.classList.contains('welcome-cta')`);
    else await page.eval(`document.querySelector('.welcome-cta').click()`);
    if (j.kb) await keys.enter(page);
  } else if (j.kb) {
    await tabTo(j, `el.getAttribute('aria-label') === ${JSON.stringify(j.t("New connection"))}`);
    await keys.enter(page);
  } else {
    await page.eval(`document.querySelector('.sidebar-header button[title=${JSON.stringify(j.t("New connection"))}]').click()`);
  }
}

/** Saves the open connection form: Enter in the last field with the keyboard (as 5.1 says), the button otherwise. */
export async function saveForm(j) {
  if (j.kb && (await j.page.eval(`document.activeElement.tagName === 'INPUT'`))) await keys.enter(j.page);
  else await pressButton(j, `document.querySelector('.connection-dialog')`, "Save");
  await j.page.waitFor(`document.querySelector('.connection-dialog') === null`, 15_000);
}

/** Replaces a cell's text through the grid: F2, the text, Enter; then Ctrl+S (keyboard) or Save. */
export async function editFirstName(j, text) {
  const { page } = j;
  // The grid is a canvas: it takes the focus without a ring, the focused cell shows it.
  if (j.kb) await tabTo(j, `el.classList.contains('grid-scroll')`, 120, { ring: false });
  else await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  await keys.right(page);
  // The grid may still be taking the focus: F2 again until the editor opens.
  await pressF2(page, ".cell-editor");
  await page.eval(`document.querySelector('.cell-editor').focus(); document.querySelector('.cell-editor').select()`);
  await page.type(text);
  await keys.enter(page);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
  await page.key("s", "KeyS", 83, CTRL);
}

export async function run(page, ctx, mode) {
  const j = journey(page, mode);
  const lang = mode === "it" ? "it" : "en";

  // 1. The home screen of an empty profile, in the language of the profile.
  assert.equal(await page.eval(`document.querySelectorAll('.tree-row').length`), 0);
  assert.deepEqual(savedConnections(ctx.dir), {});
  assert.equal(await page.eval(`document.documentElement.lang`), lang);
  // The line comes once the saved connections are read.
  await page.waitFor(`document.querySelector('.welcome-lead') !== null`);
  assert.equal(await page.eval(`document.querySelector('.welcome-lead').textContent`), j.t("A SQL client for your databases and data files. Add a connection to get started."));
  await screenIsItalian(page, mode, "the home screen", ["PlatypusDB"]);

  // 2. A PostgreSQL connection: the wrong password first.
  await newConnection(j, true);
  await pickDriver(j, "PostgreSQL");
  await page.waitFor(`document.querySelector('.connection-dialog') !== null`, 10_000);
  await screenIsItalian(page, mode, "the connection form");
  await setField(j, ".connection-dialog", "Name", PG);
  await setField(j, ".connection-dialog", "Port", "55432");
  await setField(j, ".connection-dialog", "Database", "platypus");
  await setField(j, ".connection-dialog", "User", "platypus");
  await setField(j, ".connection-dialog", "Password", "wrong-password");
  const bad = await testConnection(j);
  assert.equal(bad.ok, false, "a wrong password connected");
  assert.match(bad.text, /password|autentic|authentic/i, `the error is not readable: ${bad.text}`);
  assert.doesNotMatch(bad.text, /undefined|\[object|<[a-z]+>|^Error: Error/);
  assert.ok(bad.text.length > 15 && bad.text.length < 400, `the error looks odd: ${bad.text}`);
  await screenIsItalian(page, mode, "the connection form with an error");
  assert.deepEqual(savedConnections(ctx.dir), {}, "the test saved the connection");
  // The right one.
  await setField(j, ".connection-dialog", "Password", "platypus");
  const good = await testConnection(j);
  assert.equal(good.ok, true, good.text);
  assert.ok(good.text.startsWith(j.t("Connected")), good.text);
  await saveForm(j);
  const saved = savedConnections(ctx.dir)[PG];
  assert.ok(saved, "the PostgreSQL connection is not in connections.toml");
  assert.equal(saved.port, "55432");
  assert.equal(saved.driver, "postgres");
  assert.equal(saved.database, "platypus");
  assert.doesNotMatch(readFileSync(join(ctx.dir, "connections.toml"), "utf8"), /wrong-password|password\s*=\s*"platypus"/);

  // 3. A SQLite connection.
  await newConnection(j, false);
  await pickDriver(j, "SQLite");
  await page.waitFor(`document.querySelector('.connection-dialog') !== null`, 10_000);
  await setField(j, ".connection-dialog", "Name", LITE);
  await setField(j, ".connection-dialog", "Database file", join(ctx.dir, "s_jrn_first.db"));
  await saveForm(j);
  assert.equal(savedConnections(ctx.dir)[LITE]?.driver, "sqlite");
  await page.waitFor(`document.querySelectorAll('.tree-row').length >= 2`);
  await screenIsItalian(page, mode, "the navigator with two connections");

  // 4. A table of PostgreSQL: its data, a cell edited and saved.
  await treeOpen(j, [PG, "public", j.t("Tables"), TABLE], true);
  await waitRows(j, 3);
  await screenIsItalian(page, mode, "a table's data");
  await editFirstName(j, "alpha2");
  await until(() => psql(isPg(TABLE)), "alpha2,beta,gamma", "the edited cell was not saved");

  // 5. A query of its own, exported to CSV.
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`document.querySelectorAll('.tab').length === 2 && ${visibleTab}?.querySelector('.cm-content') === document.activeElement`, 15_000);
  await page.type(SQL);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === ${JSON.stringify(SQL)}`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await waitRows(j, 3);
  const csv = join(ctx.dir, "first.csv");
  await exportCsv(j, csv);
  assert.equal(readFileSync(csv, "utf8").replace(/\r\n/g, "\n"), "name,price\nalpha2,1.50\nbeta,2.25\ngamma,\n");
  const titles = await page.eval(tabTitles);
  assert.equal(titles.length, 2);

  // 6. Restart: the tabs are back and the saved password is used, not asked.
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes(${JSON.stringify(TABLE)})`);
  await restartApp(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`document.querySelectorAll('.tab').length === 2`, 20_000);
  assert.deepEqual(await page.eval(tabTitles), titles);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent === ${JSON.stringify(SQL)}`, 15_000);
  await treeOpen(j, [PG, "public", j.t("Tables"), TABLE]);
  assert.equal(await page.eval(`document.querySelector('.modal input[type=password]')`), null, "the saved password was asked again");
  await treeOpen(j, [LITE, "main", j.t("Tables"), "notes"]);
  assert.equal(await page.eval(`document.querySelector('.modal input[type=password]')`), null);
  assert.deepEqual(Object.keys(savedConnections(ctx.dir)).sort(), [LITE, PG].sort());

  // 7. The language, changed in the Settings: the interface follows, and after a restart too.
  const other = lang === "it" ? "en" : "it";
  await openSettings(j);
  await setSettingSelect(j, "appearance.language", "Language", other);
  const j2 = journey(page, other === "it" ? "it" : "en");
  await page.waitFor(`document.documentElement.lang === ${JSON.stringify(other)}`, 10_000);
  await screenIsItalian(page, other === "it" ? "it" : "en", "the Settings after the language change");
  await until(() => readFileSync(join(ctx.dir, "settings.toml"), "utf8").match(/^language\s*=\s*"(\w+)"/m)?.[1], other, "settings.toml does not hold the new language");
  await closeSettings(j2);
  await restartApp(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  assert.equal(await page.eval(`document.documentElement.lang`), other);
  await page.waitFor(`document.querySelectorAll('.tab').length === 2`, 20_000);
  const newConnectionLabel = j2.t("New connection");
  assert.equal(await page.eval(`document.querySelector('.sidebar-header button[title=${JSON.stringify(newConnectionLabel)}]') !== null`), true, "the navigator is not in the new language");
  assert.equal(await page.eval(`document.querySelector('button[aria-label=${JSON.stringify(j2.t("Settings"))}]') !== null`), true);
  await screenIsItalian(page, other === "it" ? "it" : "en", "the main screen after the restart");
  await openSettings(j2);
  await screenIsItalian(page, other === "it" ? "it" : "en", "the Settings after the restart");
  await closeSettings(j2);
  // The tables are still what the journey left.
  assert.equal(psql(isPg(TABLE)), "alpha2,beta,gamma");
}

/** The result's Export… button, the dialog, CSV, and the file the save dialog is answered with. */
export async function exportCsv(j, path) {
  const { page } = j;
  await answerSave(page, path);
  const title = j.t("Export result");
  const button = `[...${visibleTab}.querySelectorAll('button.link')].find(b => b.textContent === ${JSON.stringify(j.t("Export…"))})`;
  if (j.kb) await tabTo(j, `el.tagName === 'BUTTON' && el.textContent === ${JSON.stringify(j.t("Export…"))}`, 120);
  else await page.eval(`${button}.click()`);
  if (j.kb) await keys.enter(page);
  const dialog = `[...document.querySelectorAll('.modal')].find(m => m.querySelector('h2')?.textContent === ${JSON.stringify(title)})`;
  await page.waitFor(`${dialog} !== undefined`, 10_000);
  await screenIsItalian(page, j.mode, "the export dialog");
  await selectEl(j, `${dialog}.querySelector('select')`, "csv");
  await pressButton(j, dialog, "Export…");
  const done = fmt(j, "Exported {n} rows", { n: 3 });
  await page.waitFor(`[...${visibleTab}.querySelectorAll('span')].some(s => s.textContent.startsWith(${JSON.stringify(done)}))`, 60_000);
}
