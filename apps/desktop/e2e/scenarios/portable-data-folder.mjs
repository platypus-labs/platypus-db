// A portable copy: the runner copies the executable into a folder with a `data` folder next to
// it (and no PLATYPUS_DATA_DIR). The app says it is portable, keeps its connections, scripts,
// settings and history in `data` and writes nothing into the user's own profile folder; after a
// restart, and after the executable is replaced by another copy (a portable update), the
// connection and the script are still there.
import assert from "node:assert/strict";
import { copyFileSync, existsSync, readFileSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { connect, CTRL } from "../cdp.mjs";
import { palette } from "../plans.mjs";
import { runQuery, treeRow, visibleTab } from "../query.mjs";

export const portable = true;

const NAME = "Portable db";
const text = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const script = `document.querySelector('.script-row[data-path="portable_note.sql"]')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "p.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t (v) VALUES ('one'), ('two');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000f01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "p.db")}'
save_password = false
`,
  );
  mkdirSync(join(dir, "scripts", "all"), { recursive: true });
  writeFileSync(join(dir, "scripts", "all", "portable_note.sql"), "select 1");
}

/** What the user's own profile folder holds, as the app would use it without the `data` folder. */
function osProfile() {
  const folder = join(process.env.APPDATA ?? "", "dev.platypusdb.desktop");
  const marks = {};
  for (const name of ["connections.toml", "settings.toml", "history.db"]) {
    const file = join(folder, name);
    marks[name] = existsSync(file) ? statSync(file).mtimeMs : null;
  }
  return marks;
}

async function openScript(page) {
  await page.waitFor(`${script} !== null`, 15_000);
  await page.eval(`${script}.id = 'e2e-script'`);
  await page.clickSel("#e2e-script", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.includes('portable_note.sql') === true`);
}

const info = (page) => page.eval(`window.__TAURI_INTERNALS__.invoke('app_info')`);

/** Ends the app of the scenario and starts it again, as a user who quits and reopens it. */
async function restart(ctx, before = () => {}) {
  spawnSync("taskkill", ["/pid", String(ctx.app.pid), "/t", "/f"], { stdio: "ignore" });
  await new Promise((r) => setTimeout(r, 3000));
  before();
  const app = spawn(ctx.exe, [], { env: ctx.env, stdio: "ignore" });
  ctx.app = app;
  const page = await connect(ctx.port, 60_000);
  await page.waitFor(`document.querySelector('.tree-row') !== null`, 60_000);
  return page;
}

export default async function (first, ctx) {
  const { dir } = ctx;
  const os = osProfile();
  let page = first;
  try {
    // It knows it is portable, and its profile is the folder next to the executable.
    assert.equal((await info(page)).portable, true);
    assert.ok(existsSync(join(ctx.root, "app", "data")), "the data folder is next to the executable");

    // A query: the history goes to `data`; the script is saved there too.
    await page.waitFor(`${treeRow(NAME)} !== undefined`);
    await openScript(page);
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await page.key("End", "End", 35, CTRL);
    await page.type(" as portable_ok");
    await page.waitFor(`${text} === 'select 1 as portable_ok'`);
    await page.key("s", "KeyS", 83, CTRL);
    await page.waitFor(`!document.querySelector('.tab.active').textContent.includes('•')`, 10_000);
    assert.equal(readFileSync(join(dir, "scripts", "all", "portable_note.sql"), "utf8"), "select 1 as portable_ok");
    await runQuery(page, "SELECT v FROM t ORDER BY id");
    // A setting changed in the app lands in `data`.
    await page.eval(`window.__TAURI_INTERNALS__.invoke('set_setting', { key: 'layout.navigator_side', value: 'right' }).catch(() => {})`);
    for (let i = 0; i < 50 && !existsSync(join(dir, "history.db")); i++) await page.sleep(100);
    assert.ok(existsSync(join(dir, "history.db")), "the history is in the data folder");
    assert.ok(existsSync(join(dir, "settings.toml")), "the settings are in the data folder");

    // Nothing of this went to the user's own profile.
    assert.deepEqual(osProfile(), os, "the user's own profile folder was not touched");

    // Reopened: the connection and the script are where they were.
    page = await restart(ctx);
    assert.equal((await info(page)).portable, true);
    await page.waitFor(`${treeRow(NAME)} !== undefined`);
    await page.waitFor(`${script} !== null`, 15_000);
    await openScript(page);
    await page.waitFor(`${text} === 'select 1 as portable_ok'`);

    // A portable update replaces the executable: the data stays.
    page = await restart(ctx, () => copyFileSync(ctx.sourceExe, ctx.exe));
    await page.waitFor(`${treeRow(NAME)} !== undefined`);
    await page.waitFor(`${script} !== null`, 15_000);
    assert.match(readFileSync(join(dir, "connections.toml"), "utf8"), /Portable db/);
    assert.deepEqual(osProfile(), os, "still nothing in the user's own profile folder");
    await palette(page, "Settings");
    await page.waitFor(`document.querySelector('.modal.settings') !== null`, 10_000);
    await page.key("Escape", "Escape", 27);
  } finally {
    if (page !== first) {
      spawnSync("taskkill", ["/pid", String(ctx.app.pid), "/t", "/f"], { stdio: "ignore" });
      page.close();
    }
  }
}
