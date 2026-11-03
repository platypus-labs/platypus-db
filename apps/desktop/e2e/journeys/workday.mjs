// Complete user journey 3, a working day (docs/COLLAUDO.md 5.3): three connections, two editors
// and a table open; a script saved, renamed and found again; an old query reopened from the
// history; an open transaction that warns when the window is closed (and rolls back when it is);
// the zoom and the theme changed and kept after a restart. Run in English, in Italian and
// keyboard only. The databases are SQLite files: no server is needed.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import {
  answerAndExit, reopenApp, restartApp, fmt, journey, keys, palette, pressButton, runSql, screenIsItalian, tabTo, treeMenu, treeOpen, typeSql, until, visibleTab, waitRows,
} from "../journey-day.mjs";
import { setLanguage } from "../journey.mjs";
import { closeApp } from "../qa-jrn.mjs";
import { editorText } from "../query.mjs";


const tabs = `[...document.querySelectorAll('.tab')].map(t => t.textContent.replace(/×$/, ''))`;
const toolbar = `${visibleTab}.querySelector('.toolbar')`;
const settings = (dir) => readFileSync(join(dir, "settings.toml"), "utf8");

const lite = (dir, file) => join(dir, file);
function seed(path, sql) {
  const db = new DatabaseSync(path);
  db.exec(sql);
  db.close();
}

export function setup({ dir }, mode) {
  seed(lite(dir, "day-a.db"), "CREATE TABLE s_jrn_sales (id INTEGER PRIMARY KEY, item TEXT, qty INTEGER); INSERT INTO s_jrn_sales VALUES (1,'pen',3),(2,'ink',5),(3,'pad',2);");
  seed(lite(dir, "day-b.db"), "CREATE TABLE s_jrn_stock (id INTEGER PRIMARY KEY, item TEXT); INSERT INTO s_jrn_stock VALUES (1,'pen');");
  seed(lite(dir, "day-c.db"), "CREATE TABLE s_jrn_log (msg TEXT);");
  const block = (n, name, file) =>
    `\n[[connections]]\nid = "00000000-0000-4000-8000-0000000a90${n}0"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${lite(dir, file)}'\nsave_password = false\n`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n${block(1, "Day A", "day-a.db")}${block(2, "Day B", "day-b.db")}${block(3, "Day C", "day-c.db")}`,
  );
  setLanguage(dir, mode);
}

const countLog = (dir) => {
  const db = new DatabaseSync(lite(dir, "day-c.db"), { readOnly: true });
  try {
    return db.prepare("SELECT count(*) AS n FROM s_jrn_log").get().n;
  } finally {
    db.close();
  }
};

export async function run(page, ctx, mode) {
  const j = journey(page, mode);
  const T = j.t("Tables");
  const baseRatio = await page.eval(`window.devicePixelRatio`);
  const zoom = () => page.eval(`Math.round(window.devicePixelRatio / ${baseRatio} * 100)`);
  await screenIsItalian(page, mode, "the navigator at the start", ["PlatypusDB"]);

  // 1. Three connections open, two editors, a table.
  await treeOpen(j, ["Day A", "main", T, "s_jrn_sales"], true);
  await waitRows(j, 3);
  await screenIsItalian(page, mode, "a table's data");
  await treeOpen(j, ["Day B", "main", T, "s_jrn_stock"]);
  await treeOpen(j, ["Day C", "main", T, "s_jrn_log"]);
  assert.equal(await page.eval(`document.querySelector('.modal') === null`), true);
  await palette(j, fmt(j, "Open SQL editor: {name}", { name: "Day A" }));
  await runSql(j, "SELECT item, qty FROM s_jrn_sales ORDER BY id", { rows: 3 });
  await palette(j, fmt(j, "Open SQL editor: {name}", { name: "Day B" }));
  await runSql(j, "SELECT 'old query one' AS q", { rows: 1 });
  await runSql(j, "SELECT 'newer query two' AS q", { rows: 1 });
  assert.equal((await page.eval(tabs)).length, 3);
  await screenIsItalian(page, mode, "an editor with its result");

  // 2. A script: saved on the connection, renamed in the Scripts panel, found again.
  const SCRIPT = "SELECT item FROM s_jrn_sales WHERE qty > 2 ORDER BY item";
  await treeMenu(j, ["Day A"], "New", "New saved script");
  await page.waitFor(`document.querySelectorAll('.tab').length === 4`, 15_000);
  await typeSql(j, SCRIPT);
  await page.key("s", "KeyS", 83, CTRL);
  const scripts = join(ctx.dir, "scripts", "all");
  await until(() => existsSync(join(scripts, "Day A.sql")) && readFileSync(join(scripts, "Day A.sql"), "utf8"), SCRIPT, "the script was not saved to its file");
  await page.key("Enter", "Enter", 13, CTRL);
  await waitRows(j, 2);
  // Renamed with F2 on its row in the panel.
  const row = `[...document.querySelectorAll('.script-row')].find(r => r.dataset.path === 'Day A.sql')`;
  await page.waitFor(`${row} !== undefined`, 10_000);
  if (j.kb) {
    await tabTo(j, `el.classList.contains('script-row') && el.dataset.path === 'Day A.sql'`, 120);
    await page.key("F2", "F2", 113);
  } else {
    await page.eval(`${row}.focus()`);
    await page.key("F2", "F2", 113);
  }
  await page.waitFor(`document.querySelector('.script-rename') === document.activeElement`, 5_000);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("Weekly sales");
  await keys.enter(page);
  await until(() => readdirSync(scripts).filter((f) => f.endsWith(".sql")).sort().join(","), "Weekly sales.sql", "the renamed script is not the only file");
  assert.equal(readFileSync(join(scripts, "Weekly sales.sql"), "utf8"), SCRIPT);
  await page.waitFor(`[...document.querySelectorAll('.script-row .tree-label')].some(l => l.textContent === 'Weekly sales')`, 10_000);
  await screenIsItalian(page, mode, "the Scripts panel");
  // The editor tab open on the script follows the rename.
  assert.equal(await page.eval(`document.querySelector(".tab.active")?.textContent.startsWith("Weekly sales.sql")`), true);
  // Found from the global search by its new name.
  await palette(j, "Weekly sales");
  await page.waitFor(`document.querySelectorAll('.tab').length === 5`, 10_000);
  assert.equal(await page.eval(editorText), SCRIPT);

  // 3. An old query from the history comes back whole.
  await palette(j, "old query one", "old query one");
  await page.waitFor(`document.querySelectorAll('.tab').length === 6`, 10_000);
  assert.equal(await page.eval(editorText), "SELECT 'old query one' AS q");
  await screenIsItalian(page, mode, "the reopened query");

  // 4. An open transaction: closing the window asks, Cancel keeps everything, Close rolls back.
  await palette(j, fmt(j, "Open SQL editor: {name}", { name: "Day C" }));
  await runSql(j, "BEGIN;\nINSERT INTO s_jrn_log VALUES ('tx');", { script: true });
  const txLabel = j.t("Transaction open {time}").split("{time}")[0].trim();
  await page.waitFor(`${toolbar}.textContent.includes(${JSON.stringify(txLabel)})`, 20_000);
  await screenIsItalian(page, mode, "the open transaction");
  const before = await page.eval(tabs);
  const asked = await closeApp(page);
  assert.ok(asked, "closing the window with a transaction open asked nothing");
  assert.ok(asked.includes(j.t("One tab has changes not saved or committed. Close the window and discard them?")), `the warning says: ${asked}`);
  // Cancel: the window stays, the transaction too.
  if (j.kb) await keys.escape(page);
  else await pressButton(j, `document.querySelector('.modal')`, "Cancel");
  await page.waitFor(`document.querySelector('.modal') === null`, 5_000);
  assert.equal(await page.eval(`${toolbar}.textContent.includes(${JSON.stringify(txLabel)})`), true, "Cancel ended the transaction");
  assert.deepEqual(await page.eval(tabs), before);
  assert.equal(countLog(ctx.dir), 0, "the transaction is committed");
  // Close: the app ends, and the transaction with it.
  assert.ok(await closeApp(page));
  await answerAndExit(j, ctx, "document.querySelector('.modal')", "Close");
  assert.equal(countLog(ctx.dir), 0, "the open transaction was committed by closing");
  await reopenApp(page, ctx, `document.querySelector('.tree-row') !== null`);
  await page.waitFor(`document.querySelectorAll('.tab').length === ${before.length}`, 20_000);
  assert.deepEqual(await page.eval(tabs), before);
  const txOpen = `[...document.querySelectorAll('.query-tab')].some(t => t.querySelector('.toolbar')?.textContent.includes(${JSON.stringify(txLabel)}))`;
  assert.equal(await page.eval(txOpen), false, "a transaction is open after the restart");
  assert.equal(countLog(ctx.dir), 0);
  assert.equal(await zoom(), 100);
  await page.key("=", "Equal", 187, CTRL);
  await page.waitFor(`Math.round(window.devicePixelRatio / ${baseRatio} * 100) === 110`, 5_000);
  await page.key("=", "Equal", 187, CTRL);
  await page.waitFor(`Math.round(window.devicePixelRatio / ${baseRatio} * 100) === 125`, 5_000);
  await until(() => /^zoom = "125"/m.test(settings(ctx.dir)), true, "the zoom is not in settings.toml");
  const themeButton = `el.tagName === 'BUTTON' && el.getAttribute('aria-label') === ${JSON.stringify(j.t("Change theme"))}`;
  for (let i = 0; i < 2; i++) {
    if (j.kb) {
      await tabTo(j, themeButton, 120);
      await keys.enter(page);
    } else await page.eval(`document.querySelector('button[aria-label=${JSON.stringify(j.t("Change theme"))}]').click()`);
    await page.sleep(150);
  }
  await until(() => /^theme = "platypus_dark"/m.test(settings(ctx.dir)), true, "the theme is not in settings.toml");
  assert.equal(await page.eval(`document.documentElement.dataset.theme`), "dark");
  await restartApp(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`document.querySelectorAll('.tab').length === ${before.length}`, 20_000);
  await page.waitFor(`Math.round(window.devicePixelRatio / ${baseRatio} * 100) === 125`, 10_000);
  assert.equal(await page.eval(`document.documentElement.dataset.theme`), "dark");
  assert.match(settings(ctx.dir), /^theme = "platypus_dark"/m);
  assert.equal(await page.eval(`document.documentElement.lang`), mode === "it" ? "it" : "en");
  await screenIsItalian(page, mode, "the main screen after the last restart");
}
