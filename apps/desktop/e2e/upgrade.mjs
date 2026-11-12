// Upgrade tests: what the s-upgrade-* scenarios share. A published release (the MSI of a GitHub
// release, unpacked with an administrative install, so nothing is installed or registered on
// this machine) writes a user's data on a fresh profile, is closed as a user closes it, and the
// build under test then opens the same profile: everything must be there and work. The data is
// created with the old app's own Tauri commands and its own editor; the commands and payloads
// are those of the tag (`git show <tag>:apps/desktop/src-tauri/src/...`).
import assert from "node:assert/strict";
import { spawn as spawnApp } from "node:child_process";
import { spawnSync } from "./proc.mjs";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { connect as tcp, createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL, connect } from "./cdp.mjs";
import { clearStatus, editorText, idle, status } from "./qa-grid.mjs";
import { closeApp, needsPostgres, psql, psqlTry, restart } from "./qa-jrn.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const port = Number(process.env.PLATYPUS_E2E_PORT ?? 9333);
const REPO = "platypus-db/platypus-db";

// Fixed ids, so the keychain entries they leave can be found and deleted.
const PG = "00000000-0000-4000-8000-00000050e001";
const LITE = "00000000-0000-4000-8000-00000050e002";
const PROJECT = "00000000-0000-4000-9000-00000050e001";
const NAMES = { [PG]: "S Upgrade PG", [LITE]: "S Upgrade SQLite" };
const MASTER = "master-pw-s1";
const SQL_PG = "select 'upgrade-pg' as marker, count(*) from s_upgrade_pg";
const SQL_LITE = "select 'upgrade-lite' as marker, count(*) from t";
const SCRIPTS = [
  { project: PROJECT, folder: "", name: "top", connection: PG, text: "-- first\nselect 'script-top' as marker;\n" },
  { project: PROJECT, folder: "reports", name: "monthly", connection: LITE, text: "select count(*) -- monthly\nfrom t;\nselect 'è' as accent;\n" },
  { project: null, folder: "", name: "adhoc", connection: PG, text: "select 42 as answer;\n" },
];
const SETTINGS = { "appearance.theme": "platypus_dark", "appearance.language": "it", "appearance.editor_font_size": 17 };
const DASHBOARD = JSON.stringify({ title: "S board", widgets: [] });

const invokeJs = (cmd, args = {}) => `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`;
const inv = (page, cmd, args) => page.eval(invokeJs(cmd, args));
const cmdkey = (id) => spawnSync("cmdkey", ["/list"], { encoding: "utf8" }).stdout.includes(`${id}.platypusdb`);
const forgetKeys = () => [PG, LITE].forEach((id) => spawnSync("cmdkey", [`/delete:${id}.platypusdb`], { stdio: "ignore" }));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const treeRow = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const labels = `[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`;

/** The executable of release `tag`: `{ exe }` or `{ reason }`. The MSI is downloaded once into
 *  target/e2e-releases/<tag>/ and unpacked there with `msiexec /a` (administrative install: only
 *  extracts files, writes no registry key and no shortcut). */
export function releaseExe(tag) {
  const root = resolve(here, "../../../target/e2e-releases", tag);
  const app = join(root, "app");
  const find = (dir) =>
    existsSync(dir)
      ? readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
          e.isDirectory() ? find(join(dir, e.name)) : e.name === "platypus-db.exe" ? [join(dir, e.name)] : [])
      : [];
  if (find(app).length) return { exe: find(app)[0] };
  mkdirSync(root, { recursive: true });
  const msi = join(root, `PlatypusDB_${tag.slice(1)}_x64_en-US.msi`);
  if (!existsSync(msi)) {
    const got = spawnSync("gh", ["release", "download", tag, "-R", REPO, "-p", "*x64_en-US.msi", "-D", root], { encoding: "utf8" });
    if (got.status !== 0 || !existsSync(msi)) return { reason: `release ${tag} cannot be fetched: ${(got.stderr ?? "").trim()}` };
  }
  const out = spawnSync("msiexec", ["/a", msi, "/qn", `TARGETDIR=${app}`], { encoding: "utf8" });
  if (out.status !== 0 || !find(app).length) return { reason: `msiexec /a of ${tag} failed (exit ${out.status})` };
  return { exe: find(app)[0] };
}

const portFree = () =>
  new Promise((done) => {
    const socket = tcp({ host: "127.0.0.1", port, timeout: 500 });
    socket.on("connect", () => (socket.destroy(), done(false)));
    socket.on("error", () => done(true));
    socket.on("timeout", () => (socket.destroy(), done(true)));
  });

/** Starts `exe` (an old release) on the scenario's profile and returns its page and process. */
async function launchOld(exe, ctx) {
  // As the harness does for the build under test: English, no animations, and the agents' bridge on a port of
  // its own (not the default one of an app the user has open).
  const bridge = await new Promise((done) => {
    const server = createServer().listen(0, "127.0.0.1", () => {
      const { port: free } = server.address();
      server.close(() => done(free));
    });
  });
  writeFileSync(join(ctx.dir, "settings.toml"), `[appearance]\nlanguage = "en"\nanimations = "off"\n\n[agents]\nbridge_port = ${bridge}\nconfirm_writes = false\n`);
  const deadline = Date.now() + 20_000;
  while (!(await portFree()) && Date.now() < deadline) await sleep(200);
  const app = spawnApp(exe, [], {
    env: { ...process.env, PLATYPUS_DATA_DIR: ctx.dir, WEBVIEW2_USER_DATA_FOLDER: join(ctx.root, "webview"), PLATYPUS_DEVTOOLS_PORT: String(port) },
    stdio: "ignore",
  });
  const page = await connect(port, 60_000);
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`, 60_000);
  return { page, app };
}

/** Types `sql` in a new query tab of the connection `name` and runs it. */
async function runInNewTab(page, name, sql) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow(name)}.getAttribute('aria-expanded') === 'true'`, 30_000);
  // Double click on a connection: a SQL editor on it.
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  await page.eval(`document.querySelectorAll('#s-conn').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 's-conn'; document.getElementById('s-conn').scrollIntoView({ block: 'center' })`);
  await page.clickSel("#s-conn", { clickCount: 2 });
  await page.waitFor(`document.querySelectorAll('.tab').length === ${tabs + 1} && document.querySelector('.query-tab .cm-content') != null`, 15_000);
  const tab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
  await page.eval(`${tab}.querySelector('.cm-content').focus()`);
  await page.type(sql);
  await page.waitFor(`${tab}.querySelector('.cm-content').textContent === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  assert.equal(await idle(page), "1 row", await page.eval(`${tab}.querySelector('.error, .notice')?.textContent ?? ''`));
}

/** The old release writes the user's data and is closed. Returns what it wrote. */
async function oldPhase(exe, ctx, { vault, tag }) {
  forgetKeys();
  psql("DROP TABLE IF EXISTS s_upgrade_pg; CREATE TABLE s_upgrade_pg (id int PRIMARY KEY, note text); INSERT INTO s_upgrade_pg VALUES (1, 'one'), (2, 'two')");
  const lite = new DatabaseSync(join(ctx.dir, "lite.db"));
  lite.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1), (2), (3);");
  lite.close();
  const { page, app } = await launchOld(exe, ctx);
  const wrote = { version: await inv(page, "app_info", {}).then((i) => i.version ?? JSON.stringify(i)).catch(() => "?") };
  try {
    // Connections: PostgreSQL with its password in the keychain, a SQLite file.
    const pg = await inv(page, "default_connection", { driver: "postgres" });
    Object.assign(pg, { id: PG, name: NAMES[PG], host: "localhost", port: 55432, database: "platypus", user: "platypus", ssl_mode: "disable", save_password: true });
    await inv(page, "save_connection", { connection: pg, password: { action: "set", value: "platypus" }, sshSecrets: null, projects: null });
    const lt = await inv(page, "default_connection", { driver: "sqlite" });
    Object.assign(lt, { id: LITE, name: NAMES[LITE], database: join(ctx.dir, "lite.db"), save_password: false });
    await inv(page, "save_connection", { connection: lt, password: { action: "keep" }, sshSecrets: null, projects: null });
    wrote.keychainBefore = cmdkey(PG);
    // A project with both connections, a folder in it (SQLite goes into it once the tabs are made).
    await inv(page, "save_project", { project: { id: PROJECT, name: "S Upgrade Project", color: "#2f9e6e", initials: "", connections: [PG, LITE] } });
    const withFolder = await inv(page, "create_connection_folder", { projectId: PROJECT, name: "S Folder", parent: null });
    const folder = withFolder.find((p) => p.id === PROJECT).folders.find((f) => f.name === "S Folder").id;
    // Three scripts: two in the project (one in a folder of scripts), one in All connections.
    await inv(page, "create_script_folder", { project: PROJECT, path: "reports" });
    for (const s of SCRIPTS) {
      const rel = await inv(page, "create_script", { project: s.project, folder: s.folder, name: s.name, connection: s.connection });
      const { root } = await inv(page, "list_scripts", { project: s.project });
      await inv(page, "write_text_file", { path: `${root}/${rel}`, contents: s.text });
    }
    if (tag !== "v0.2.1") {
      await inv(page, "save_dashboard", { project: null, name: "s-board", contents: DASHBOARD });
      wrote.dashboard = true;
    }
    // The interface refreshes from the stores; then queries run in two tabs of their own.
    await page.eval("location.reload()");
    await page.waitFor(`${labels}.includes(${JSON.stringify(NAMES[PG])})`, 60_000);
    await runInNewTab(page, NAMES[PG], SQL_PG);
    await runInNewTab(page, NAMES[LITE], SQL_LITE);
    await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('upgrade-lite')`, 10_000);
    wrote.workspace = await page.eval(`localStorage.getItem('platypus.workspace.v2')`);
    await inv(page, "place_connection", { projectId: PROJECT, connectionId: LITE, folderId: folder });
    for (const [key, value] of Object.entries(SETTINGS)) await inv(page, "set_setting", { key, value });
    if (vault) {
      await inv(page, "enable_vault", { password: MASTER });
      wrote.keychainAfterVault = cmdkey(PG);
    }
    const asked = await closeApp(page);
    assert.equal(asked, null, `closing the old app asked: ${asked}`);
  } finally {
    page.close();
    spawnSync("taskkill", ["/pid", String(app.pid), "/t", "/f"], { stdio: "ignore" });
  }
  const history = new DatabaseSync(join(ctx.dir, "history.db"), { readOnly: true });
  wrote.historyVersion = history.prepare("PRAGMA user_version").get().user_version;
  wrote.historyRows = history.prepare("SELECT sql FROM history ORDER BY id").all().map((r) => r.sql);
  history.close();
  wrote.connectionsVersion = readFileSync(join(ctx.dir, "connections.toml"), "utf8").match(/^version = (\d+)/m)?.[1];
  return wrote;
}

/** Everything the old release wrote (plus `more`, what the new one added), read through the UI and
 *  the commands of the build under test. */
async function verify(page, ctx, { vault, more = false }) {
  await page.waitFor(`${labels}.includes(${JSON.stringify(NAMES[PG])})`, 60_000);
  // Settings: language and theme.
  const settings = await inv(page, "get_settings", {});
  assert.equal(settings.appearance?.theme, SETTINGS["appearance.theme"], JSON.stringify(settings));
  assert.equal(settings.appearance?.language, "it");
  assert.equal(settings.appearance?.editor_font_size, 17);
  assert.equal(await page.eval(`document.documentElement.lang`), "it");
  // Connections and the project.
  const conns = await inv(page, "list_connections", {});
  const pg = conns.find((c) => c.id === PG);
  const lite = conns.find((c) => c.id === LITE);
  assert.deepEqual([pg?.name, pg?.driver, pg?.host, pg?.port, pg?.database, pg?.user, pg?.save_password], [NAMES[PG], "postgres", "localhost", 55432, "platypus", "platypus", true]);
  assert.deepEqual([lite?.name, lite?.driver, lite?.database], [NAMES[LITE], "sqlite", join(ctx.dir, "lite.db")]);
  const [project] = await inv(page, "list_projects", {});
  assert.deepEqual([project.name, project.connections], ["S Upgrade Project", [PG, LITE]]);
  assert.deepEqual(project.folders.map((f) => f.name), ["S Folder"]);
  assert.equal(project.placement[LITE], project.folders[0].id);
  // Scripts: names, links and exact text, in the project and in All connections.
  for (const project of [PROJECT, null]) {
    const { root, entries } = await inv(page, "list_scripts", { project });
    const mine = SCRIPTS.filter((s) => s.project === project);
    const files = entries.filter((e) => !e.folder).map((e) => [e.path, e.connection]).sort();
    const want = mine.map((s) => [`${s.folder ? `${s.folder}/` : ""}${s.name}.sql`, s.connection]);
    if (more && project === PROJECT) want.push(["added-by-new.sql", LITE]);
    assert.deepEqual(files, want.sort());
    for (const s of mine) {
      const text = await inv(page, "read_text_file", { path: `${root}/${s.folder ? `${s.folder}/` : ""}${s.name}.sql` });
      assert.equal(text, s.text);
    }
  }
  if (ctx.old.dashboard) {
    assert.equal(await inv(page, "read_dashboard", { project: null, name: "s-board" }), DASHBOARD);
  }
  // History: the two queries the old app ran, still ok.
  const history = await inv(page, "search_history", { connectionIds: null, text: null, beforeId: null, origin: null, limit: 50 });
  for (const sql of [SQL_PG, SQL_LITE]) assert.ok(history.some((h) => h.sql === sql && h.status === "ok"), `no history for ${sql}`);
  // Tabs: the same two, with the text of each.
  await page.waitFor(`document.querySelectorAll('.tab').length >= 2`, 20_000);
  const texts = [];
  const tabCount = await page.eval(`document.querySelectorAll('.tab').length`);
  for (let i = 0; i < tabCount; i++) {
    await page.eval(`document.querySelectorAll('.tab')[${i}].click()`);
    await page.waitFor(`[...document.querySelectorAll('.query-tab')].some(t => t.style.display !== 'none' && t.querySelector('.cm-content'))`, 10_000);
    texts.push(await page.eval(editorText));
  }
  assert.ok(texts.includes(SQL_PG) && texts.includes(SQL_LITE), JSON.stringify(texts));
  assert.equal(tabCount, 2);
  await connectsAndRuns(page, vault);
  // The log has no error line.
  const log = readFileSync(join(ctx.dir, "logs", "platypus.log"), "utf8");
  assert.match(log, /PlatypusDB \S+ started/);
  assert.doesNotMatch(log, /^\S+ (ERROR|WARN|PANIC)/m, log);
}

/** PostgreSQL connects without a prompt (keychain), or after the master password (vault); the
 *  SQLite one opens; the restored PostgreSQL tab runs its query. */
async function connectsAndRuns(page, vault) {
  await page.waitFor(`${treeRow(NAMES[PG])} !== undefined`);
  const state = await inv(page, "vault_status", {});
  if (vault) {
    assert.deepEqual(state, { enabled: true, unlocked: false });
    await page.eval(`${treeRow(NAMES[PG])}.querySelector('.twisty').click()`);
    await page.waitFor(`document.querySelector('.unlock-vault') !== null`, 15_000);
    const type = (text) => page.eval(`(() => {
      const input = document.querySelector('.unlock-vault input[type=password]');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(text)});
      input.dispatchEvent(new Event('input', { bubbles: true }));
      document.querySelector('.unlock-vault button[type=submit]').click();
    })()`);
    await type("not-the-master");
    await page.waitFor(`document.querySelector('.unlock-vault .notice.error') !== null`, 15_000);
    await type(MASTER);
    await page.waitFor(`document.querySelector('.unlock-vault') === null`, 15_000);
    assert.equal((await inv(page, "vault_status", {})).unlocked, true);
  } else {
    assert.equal(state.enabled, false);
    await page.eval(`${treeRow(NAMES[PG])}.querySelector('.twisty').click()`);
  }
  await page.waitFor(`${treeRow(NAMES[PG])}.getAttribute('aria-expanded') === 'true' && ${treeRow("platypus")} !== undefined || document.querySelector('.modal input[type=password]') !== null`, 30_000);
  assert.equal(await page.eval(`document.querySelector('.modal input[type=password]')`), null, "the saved password was asked again");
  assert.equal(await inv(page, "connection_has_password", { connectionId: PG }), true);
  const opened = await page.eval(`${invokeJs("open_session", { connectionId: LITE, password: null, database: null, side: true })}.then(() => 'ok', e => JSON.stringify(e))`);
  assert.equal(opened, "ok");
  // The restored PostgreSQL tab runs.
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  for (let i = 0; i < tabs; i++) {
    await page.eval(`document.querySelectorAll('.tab')[${i}].click()`);
    await page.waitFor(`[...document.querySelectorAll('.query-tab')].some(t => t.style.display !== 'none' && t.querySelector('.cm-content'))`, 10_000);
    if ((await page.eval(editorText)) !== SQL_PG) continue;
    await page.eval(`[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none').querySelector('.cm-content').focus()`);
    await clearStatus(page);
    await page.key("Enter", "Enter", 13, CTRL);
    await page.waitFor(`/^1 (row|riga)/.test(${status})`, 30_000);
    return;
  }
  throw new Error("the PostgreSQL tab did not come back");
}

/** Data written by the build under test, read again after its own restart. */
async function overItself(page, ctx) {
  const { root } = await inv(page, "list_scripts", { project: PROJECT });
  const rel = await inv(page, "create_script", { project: PROJECT, folder: "", name: "added-by-new", connection: LITE });
  await inv(page, "write_text_file", { path: `${root}/${rel}`, contents: "select 'added' as by_new;\n" });
  await inv(page, "set_setting", { key: "appearance.editor_font_size", value: 18 });
  const lite = (await inv(page, "list_connections", {})).find((c) => c.id === LITE);
  await inv(page, "save_connection", { connection: { ...lite, name: NAMES[LITE] }, password: { action: "keep" }, sshSecrets: null, projects: null });
  // The first save writes this build's file format.
  const version = Number(readFileSync(join(ctx.dir, "connections.toml"), "utf8").match(/^version = (\d+)/m)?.[1]);
  assert.ok(version >= Number(ctx.old.connectionsVersion), `connections.toml version ${version} < ${ctx.old.connectionsVersion}`);
  const db = new DatabaseSync(join(ctx.dir, "history.db"), { readOnly: true });
  const historyVersion = db.prepare("PRAGMA user_version").get().user_version;
  const historyRows = db.prepare("SELECT count(*) AS n FROM history").get().n;
  db.close();
  assert.ok(historyVersion >= ctx.old.historyVersion, `history schema ${historyVersion}`);
  assert.ok(historyRows >= ctx.old.historyRows.length, `history rows ${historyRows}`);
  const toml = readFileSync(join(ctx.dir, "settings.toml"), "utf8");
  assert.match(toml, /theme = "platypus_dark"/);
  assert.match(toml, /language = "it"/);
  ctx.migrated = { connectionsVersion: version, historyVersion, historyRows };
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('upgrade-pg')`, 10_000);
  await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await inv(page, "set_setting", { key: "appearance.editor_font_size", value: 17 });
}

/** The pieces of one upgrade scenario, for `export const skip = ...` and `export default`. */
export function upgradeScenario(tag, { vault = false } = {}) {
  const release = { current: null };
  return {
    skip: async () => {
      if (process.platform !== "win32") return "Windows only";
      release.current = releaseExe(tag);
      if (release.current.reason) return release.current.reason;
      return needsPostgres();
    },
    // The old release runs here, before the harness starts the build under test on the profile.
    async setup(ctx) {
      try {
        ctx.old = await oldPhase(release.current.exe, ctx, { vault, tag });
      } catch (e) {
        ctx.oldError = e;
      }
    },
    async run(page, ctx) {
      try {
        if (ctx.oldError) throw ctx.oldError;
        if (!vault) assert.equal(ctx.old.keychainBefore, true, "the old app did not save the password in the keychain");
        // The upgrade: everything the old release wrote, found by the new one.
        await verify(page, ctx, { vault });
        console.log(`     ${tag} -> new: ${JSON.stringify({ ...ctx.old, workspace: undefined })}`);
        // The new build's own data survives its own restart.
        await overItself(page, ctx);
        await verify(page, ctx, { vault, more: true });
        console.log(`     migrated: ${JSON.stringify(ctx.migrated)}`);
      } finally {
        forgetKeys();
        void psqlTry("DROP TABLE IF EXISTS s_upgrade_pg");
      }
    },
  };
}
