// Shared team projects: a folder with platypus-project.toml and scripts/ (what a team keeps in
// a repository or on a shared drive) is opened as a project, and the app and the folder follow
// each other. The folder never holds a credential, a project is shared to a folder with or
// without the user names, an unreadable file is never overwritten, a missing folder is told and
// not created, and removing the project never touches the folder. The init statements a teammate
// writes into a connection are shown and run only once approved here, and asked again when the
// text changes; ones the user writes locally never ask.
//
// The teammate is played by the scenario writing files into the folder; the second computer is
// not driven (one app runs per scenario).
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { setValue } from "../admin-users.mjs";

const SHARED = "00000000-0000-4000-9000-0000000000f1";
const MINE = "00000000-0000-4000-9000-0000000000f2";
const MINE2 = "00000000-0000-4000-9000-0000000000f3";
const C = {
  sales: "00000000-0000-4000-8000-0000000000f1",
  ops: "00000000-0000-4000-8000-0000000000f2",
  fresh: "00000000-0000-4000-8000-0000000000f3",
  mine: "00000000-0000-4000-8000-0000000000f4",
  mine2: "00000000-0000-4000-8000-0000000000f5",
};
const FOLDER = "00000000-0000-4000-a000-0000000000f1";
const CANARY = "CANARY-PW-9";

/** A shared file without its header comment (which says "no passwords") and the `save_password`
 *  flag the app writes for every connection: what is left must hold no credential. */
const data = (text) => text.replace(/^#.*$/gm, "").replace(/^save_password = .*$/gm, "");
const team = (dir) => join(dir, "team");
const projectFile = (folder) => join(folder, "platypus-project.toml");
const square = (id) => `document.querySelector('.project-square[data-project="${id}"]')`;
const toast = `([...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' | '))`;
const row = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const labels = `[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`;
const invoke = (command, args) => `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`;

/** A shared project file as a teammate wrote it, with a secret hand-edited into it. */
function teamFile(dir, { name = "Team analytics", color = "#2f9e6e", extra = "", salesInit = "" } = {}) {
  return `version = 1
id = "${SHARED}"
name = "${name}"
color = "${color}"

[[folders]]
id = "${FOLDER}"
name = "Warehouse"

[[connections]]
id = "${C.sales}"
name = "Sales"
driver = "sqlite"
database = '${join(dir, "sales.db")}'
folder = "${FOLDER}"

[connections.options]
warehouse = "WH"
password = "${CANARY}"
${salesInit ? `\n[connections.init]\nstatements = "${salesInit}"\n` : ""}
[[connections]]
id = "${C.ops}"
name = "Ops"
driver = "sqlite"
database = '${join(dir, "ops.db")}'
${extra}`;
}

export function setup({ dir }) {
  for (const f of ["sales.db", "ops.db", "fresh.db", "mine.db", "mine2.db"]) {
    const db = new DatabaseSync(join(dir, f));
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);");
    db.close();
  }
  // Two projects of the user's own, to share: one with a login in the keys and a user name.
  const mine = (id, name, db) =>
    `[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, db)}'\nuser = "alice"\nsave_password = false\npassword_command = "echo ${CANARY}"\n`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[projects]]
id = "${MINE}"
name = "Mine"
color = "#d08f20"
connections = ["${C.mine}"]

[[projects]]
id = "${MINE2}"
name = "Mine two"
color = "#a46be0"
connections = ["${C.mine2}"]

${mine(C.mine, "Mine DB", "mine.db")}
${mine(C.mine2, "Mine two DB", "mine2.db")}`,
  );
  // The scripts of the first, to be copied when it is shared.
  mkdirSync(join(dir, "scripts", MINE), { recursive: true });
  writeFileSync(join(dir, "scripts", MINE, "weekly.sql"), "SELECT 'weekly'");
  writeFileSync(join(dir, "scripts", MINE, ".links.json"), JSON.stringify({ "weekly.sql": C.mine }));
  // The team's folder.
  mkdirSync(join(team(dir), "scripts"), { recursive: true });
  writeFileSync(projectFile(team(dir)), teamFile(dir));
  writeFileSync(join(team(dir), "scripts", "daily.sql"), "SELECT 'daily'");
  writeFileSync(join(team(dir), "scripts", ".links.json"), JSON.stringify({ "daily.sql": C.sales }));
  // Another folder, for the project that is shared from the app.
  mkdirSync(join(dir, "out1"));
  mkdirSync(join(dir, "out2"));
}

/** Waits until `file` holds text that satisfies `check`; returns it. */
async function fileWith(page, file, check, what) {
  const until = Date.now() + 15_000;
  for (;;) {
    const text = existsSync(file) ? readFileSync(file, "utf8") : "";
    if (check(text)) return text;
    assert.ok(Date.now() < until, `${what}: ${text}`);
    await page.sleep(100);
  }
}

/** The window gets focus, which is when the folders of shared projects are read. */
async function focus(page) {
  await page.eval(`window.dispatchEvent(new Event('focus'))`);
}

async function answerFolder(page, folder) {
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(folder)})`);
}

async function menuItem(page, id, text) {
  const r = await page.rect(`.project-square[data-project="${id}"]`);
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.includes(${JSON.stringify(text)}))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.includes(${JSON.stringify(text)})).click()`);
}

async function openShared(page, folder) {
  await answerFolder(page, folder);
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await page.eval(`document.querySelector('.project-dialog [data-action="open-shared"]').click()`);
}

export default async function (page, { dir }) {
  await page.waitFor(`window.__platypus !== undefined && ${row("Mine DB")} !== undefined`, 15_000);

  // Opening the team's folder: the project, its folder and connections, its scripts.
  await openShared(page, team(dir));
  await page.waitFor(`${square(SHARED)} !== null`);
  assert.equal(await page.eval(`${square(SHARED)}.dataset.shared`), "true");
  assert.match(await page.eval(`${square(SHARED)}.title`), /Team analytics .*shared through/);
  assert.equal(await page.eval(`document.querySelector('.project-square.active').dataset.project`), SHARED);
  await page.waitFor(`${row("Sales")} !== undefined && ${row("Ops")} !== undefined`);
  assert.equal(await page.eval(`${row("Mine DB")} === undefined`), true, "only the project connections show");
  await page.waitFor(`document.querySelector('.script-row[data-path="daily.sql"]') !== null`);

  // No credential came in with it, even the one hand-edited into the file; the user's own is
  // asked at the first connect, and kept in the keychain after it.
  const store = readFileSync(join(dir, "connections.toml"), "utf8");
  const imported = store.slice(store.indexOf(`id = "${C.sales}"`), store.indexOf(`id = "${C.ops}"`));
  // (`save_password = false` is the app's own flag, not a credential.)
  assert.doesNotMatch(imported, new RegExp(CANARY), imported);
  assert.doesNotMatch(imported, /^\s*password(_command)?\s*=/m, imported);
  assert.match(imported, /warehouse = "WH"/);
  assert.match(store, /\[projects\.shared\]/);
  // SQLite is embedded: no password is ever asked, so none is marked as kept in the keychain
  // (the rule is tested in Rust for the drivers that sign in).
  assert.match(imported, /save_password = false/);

  // The scripts are in the shared folder: a new one is written there, and the folder's own
  // list is the app's.
  const scripts = await page.eval(invoke("create_script", { project: SHARED, folder: "", name: "adhoc", connection: C.ops }));
  assert.equal(scripts, "adhoc.sql");
  assert.equal(existsSync(join(team(dir), "scripts", "adhoc.sql")), true);
  assert.equal(existsSync(join(dir, "scripts", SHARED)), false, "nothing is kept in the app's own folder");
  assert.match(readFileSync(join(team(dir), "scripts", ".links.json"), "utf8"), new RegExp(`"adhoc.sql": "${C.ops}"`));

  // A script a teammate pulled in appears when the window gets focus.
  writeFileSync(join(team(dir), "scripts", "pulled.sql"), "SELECT 'pulled'");
  await focus(page);
  await page.waitFor(`document.querySelector('.script-row[data-path="pulled.sql"]') !== null`);

  // The same folder is not opened twice.
  await openShared(page, team(dir));
  await page.waitFor(`/already open/.test(${toast})`);
  assert.equal(await page.eval(`document.querySelectorAll('.project-square[data-shared]').length`), 1);

  // A folder without the file is refused with its name.
  mkdirSync(join(dir, "empty"));
  await openShared(page, join(dir, "empty"));
  await page.waitFor(`/platypus-project.toml is not in/.test(${toast})`);

  // What the app changes is written to the file, with no secret and in the file's layout.
  await menuItem(page, SHARED, "Rename…");
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await setValue(page, `.project-dialog input:not(.project-initials):not([type=color])`, "Team analytics 2");
  await page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);
  const renamed = await fileWith(page, projectFile(team(dir)), (t) => t.includes('name = "Team analytics 2"'), "rename not written");
  assert.doesNotMatch(renamed, new RegExp(CANARY));
  assert.doesNotMatch(data(renamed), /password|secret_id|key_path/);
  assert.match(renamed, /name = "Warehouse"/);
  assert.match(renamed, /folder = "00000000-0000-4000-a000-0000000000f1"/);
  assert.match(renamed, /^# A PlatypusDB project shared with a team\./);

  // A change made in the folder is picked up on focus, with a notice: a color, a connection.
  writeFileSync(
    projectFile(team(dir)),
    teamFile(dir, {
      name: "Team analytics 2",
      color: "#e05a5a",
      extra: `\n[[connections]]\nid = "${C.fresh}"\nname = "Fresh"\ndriver = "sqlite"\ndatabase = '${join(dir, "fresh.db")}'\n`,
    }),
  );
  await focus(page);
  await page.waitFor(`/updated from its shared folder: 1 added/.test(${toast})`);
  await page.waitFor(`${row("Fresh")} !== undefined`);
  assert.equal(await page.eval(`getComputedStyle(${square(SHARED)}).backgroundColor`), "rgb(224, 90, 90)");
  // Reading it again changes nothing and says nothing.
  await page.eval(`document.querySelectorAll('.toast').forEach((t) => t.click())`);
  await focus(page);
  await page.sleep(500);
  assert.equal(await page.eval(toast), "");

  // A file that does not read (a merge left in it) is told, kept as it is, and the app does
  // not write over it while it stays so.
  const conflict = `<<<<<<< HEAD\nversion = 1\n=======\nversion = 1\n>>>>>>> theirs\n`;
  writeFileSync(projectFile(team(dir)), conflict);
  await focus(page);
  await page.waitFor(`/cannot be read, so it is left as it is/.test(${toast})`);
  await menuItem(page, SHARED, "Rename…");
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await setValue(page, `.project-dialog input:not(.project-initials):not([type=color])`, "While broken");
  await page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') === null`);
  await page.sleep(500);
  assert.equal(readFileSync(projectFile(team(dir)), "utf8"), conflict);
  // Repaired by hand: the file is read again, and its name wins over the app's.
  writeFileSync(projectFile(team(dir)), teamFile(dir, { name: "Team analytics 3", color: "#e05a5a" }));
  await focus(page);
  await page.waitFor(`document.querySelector('.project-square[data-shared]').title.startsWith('Team analytics 3')`);

  // A folder that is gone (a drive not mounted) is told, nothing is created in its place, and
  // the project comes back with the folder.
  // Other errors about the same folder (the scripts list) replace the toast at once, so every
  // toast shown is kept.
  await page.eval(`(() => { window.__toasts = []; new MutationObserver(() => { const t = document.querySelector('.toast')?.textContent; if (t) window.__toasts.push(t); }).observe(document.body, { childList: true, subtree: true, characterData: true }); })()`);
  renameSync(team(dir), join(dir, "team-away"));
  await focus(page);
  await page.waitFor(`window.__toasts.some(t => /shared folder of Team analytics 3 is not available/.test(t))`);
  assert.equal(existsSync(team(dir)), false);
  renameSync(join(dir, "team-away"), team(dir));
  await focus(page);
  await page.waitFor(`${row("Sales")} !== undefined`);

  // Sharing a project of one's own: without the user names, the file has the connections'
  // address and nothing that logs in, and the scripts come along.
  await menuItem(page, MINE, "Share project…");
  await page.waitFor(`document.querySelector('.share-project-dialog') !== null`);
  assert.equal(await page.eval(`document.querySelector('[data-field="share-user"]').checked`), false);
  assert.equal(await page.eval(`document.querySelector('[data-action="share-submit"]').disabled`), true);
  // The team's folder holds a project already.
  await setValue(page, `.share-project-dialog [data-field="share-folder"]`, team(dir));
  await page.eval(`document.querySelector('[data-action="share-submit"]').click()`);
  await page.waitFor(`/already holds a shared project/.test(document.querySelector('[data-field="share-error"]')?.textContent ?? '')`);
  await answerFolder(page, join(dir, "out1"));
  await page.eval(`document.querySelector('[data-action="share-browse"]').click()`);
  await page.waitFor(`document.querySelector('[data-field="share-folder"]').value === ${JSON.stringify(join(dir, "out1"))}`);
  await page.eval(`document.querySelector('[data-action="share-submit"]').click()`);
  await page.waitFor(`document.querySelector('.share-project-dialog') === null && ${square(MINE)}.dataset.shared === 'true'`);
  const out1 = readFileSync(projectFile(join(dir, "out1")), "utf8");
  assert.match(out1, /name = "Mine"/);
  assert.match(out1, /name = "Mine DB"/);
  assert.doesNotMatch(data(out1), new RegExp(`${CANARY}|alice|password|secret_id|key_path`), out1);
  assert.equal(readFileSync(join(dir, "out1", "scripts", "weekly.sql"), "utf8"), "SELECT 'weekly'");
  assert.match(readFileSync(join(dir, "out1", "scripts", ".links.json"), "utf8"), new RegExp(C.mine));

  // With the user names, when asked.
  await menuItem(page, MINE2, "Share project…");
  await page.waitFor(`document.querySelector('.share-project-dialog') !== null`);
  await setValue(page, `.share-project-dialog [data-field="share-folder"]`, join(dir, "out2"));
  await page.eval(`document.querySelector('[data-field="share-user"]').click()`);
  await page.waitFor(`document.querySelector('[data-field="share-user"]').checked`);
  await page.eval(`document.querySelector('[data-action="share-submit"]').click()`);
  await page.waitFor(`${square(MINE2)}.dataset.shared === 'true'`);
  const out2 = readFileSync(projectFile(join(dir, "out2")), "utf8");
  assert.match(out2, /user = "alice"/);
  assert.doesNotMatch(data(out2), new RegExp(`${CANARY}|password`));
  // A shared project has no "Share project…" any more.
  const r = await page.rect(`.project-square[data-project="${MINE}"]`);
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.equal(await page.eval(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.includes('Share project'))`), false);
  assert.equal(await page.eval(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.includes('Read the shared folder again'))`), true);
  await page.key("Escape", "Escape", 27);

  // The init statements of a shared connection run only after this machine's user read them.
  await page.eval(`window.__platypus.switchProject(${JSON.stringify(SHARED)})`);
  await page.waitFor(`${row("Sales")} !== undefined`);
  const tables = () => {
    const db = new DatabaseSync(join(dir, "sales.db"));
    const names = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").all().map((r) => r.name);
    db.close();
    return names;
  };
  const dialog = `document.querySelector('[data-testid="init-approval"]')`;
  const sqlShown = `(document.querySelector('[data-testid="init-approval-sql"]')?.textContent ?? '')`;
  const answerInit = async (label) => {
    await page.waitFor(`${dialog} !== null`, 15_000);
    await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
    await page.waitFor(`${dialog} === null`);
  };
  /** A new SQL editor on Sales that runs a statement, which is what opens its session (the
   *  navigator's own session, kept from before, is not asked again); returns how many tabs
   *  there were before. */
  const openEditor = async () => {
    const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
    await page.eval(`${row("Sales")}.querySelector('button[title="Open SQL editor"]').click()`);
    await page.waitFor(`document.querySelectorAll('.query-tab').length === ${tabs + 1}`, 15_000);
    const tab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
    await page.eval(`${tab}.querySelector('.cm-content').focus()`);
    await page.type("SELECT 1");
    await page.waitFor(`/SELECT 1/.test(${tab}.querySelector('.cm-content').textContent)`);
    await page.key("Enter", "Enter", 13, CTRL);
    return tabs;
  };
  const visibleStatus = `([...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')?.querySelector('.statusbar span')?.textContent ?? '')`;
  const connect = `${invoke("open_session", { connectionId: C.sales, password: null, database: null, side: false, init: null })}.then(() => 'connected', (e) => e)`;
  const setInit = async (statements) => {
    writeFileSync(projectFile(team(dir)), teamFile(dir, { name: "Team analytics 3", color: "#e05a5a", salesInit: statements }));
    await focus(page);
    await fileWith(page, join(dir, "connections.toml"), (t) => t.includes(statements), "init not read");
  };
  await setInit("CREATE TABLE IF NOT EXISTS marker_one (id INTEGER)");
  // The backend refuses to connect and hands over the text; nothing ran.
  const refused = await page.eval(connect);
  assert.equal(refused.kind, "init_approval");
  assert.match(refused.message, /marker_one/);
  assert.equal(tables().includes("marker_one"), false);
  // In the app the dialog shows the whole text, read-only; connecting without it runs nothing.
  await openEditor();
  await page.waitFor(`${dialog} !== null`, 15_000);
  assert.match(await page.eval(sqlShown), /CREATE TABLE IF NOT EXISTS marker_one \(id INTEGER\)/);
  assert.equal(await page.eval(`document.querySelector('[data-testid="init-approval-sql"]').matches('pre')`), true);
  await answerInit("Connect without them");
  await page.sleep(1000);
  assert.equal(tables().includes("marker_one"), false, "skipped, so not run");
  // Skipping remembers nothing: the next connection asks again; approving runs the text.
  await page.sleep(2500);
  await openEditor();
  await answerInit("Run them and remember");
  const until = Date.now() + 15_000;
  while (!tables().includes("marker_one")) {
    assert.ok(Date.now() < until, "approved statements did not run");
    await page.sleep(100);
  }
  // Approved text does not ask again.
  await page.sleep(2500);
  await openEditor();
  await page.waitFor(`/^1 row/.test(${visibleStatus})`, 15_000);
  assert.equal(await page.eval(`${dialog} === null`), true, "approved text asks no more");
  assert.equal(await page.eval(connect), "connected");
  // The approval stays on this machine: the shared file has nothing of it.
  assert.doesNotMatch(readFileSync(projectFile(team(dir)), "utf8"), /approved/);
  // Changed text asks again, and "without" skips it.
  await setInit("CREATE TABLE IF NOT EXISTS marker_two (id INTEGER)");
  await page.sleep(2500);
  await openEditor();
  await page.waitFor(`/marker_two/.test(${sqlShown})`, 15_000);
  await answerInit("Connect without them");
  await page.sleep(500);
  assert.equal(tables().includes("marker_two"), false);
  // Statements the user wrote here never ask: saving the connection with new text approves it.
  const sales = (await page.eval(invoke("list_connections", {}))).find((c) => c.id === C.sales);
  sales.init.statements = "CREATE TABLE marker_local (id INTEGER)";
  await page.eval(invoke("save_connection", { connection: sales, password: { action: "keep" }, sshSecrets: null, projects: null }));
  assert.equal(await page.eval(connect), "connected");
  assert.equal(tables().includes("marker_local"), true);

  // Removing a shared project from the app leaves its folder and files alone, and the
  // connections stay saved.
  const before = readFileSync(projectFile(team(dir)), "utf8");
  await menuItem(page, SHARED, "Delete project");
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.confirm').textContent`), /shared folder and its files are not deleted/);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Remove').click()`);
  await page.waitFor(`${square(SHARED)} === null`);
  assert.equal(readFileSync(projectFile(team(dir)), "utf8"), before);
  assert.equal(existsSync(join(team(dir), "scripts", "daily.sql")), true);
  assert.match(readFileSync(join(dir, "connections.toml"), "utf8"), new RegExp(`id = "${C.sales}"`));
  assert.doesNotMatch(readFileSync(join(dir, "connections.toml"), "utf8"), /Team analytics/);

  // The folder opens again: the project is back with the same connections, not copies.
  await openShared(page, team(dir));
  await page.waitFor(`${square(SHARED)} !== null`);
  const names = await page.eval(`${labels}.filter(l => l === 'Sales')`);
  assert.equal(names.length, 1);
}
