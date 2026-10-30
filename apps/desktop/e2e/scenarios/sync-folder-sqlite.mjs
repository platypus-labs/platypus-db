// Encrypted folder sync (Settings > Sync): the folder and the sync passphrase are chosen, the
// app writes platypus-sync.bin there, and nothing readable is in it: no password command, no
// connection name, no script text, no setting. The machine's own settings (drivers) are left
// out of what is remembered. Sync now, the sync at start, local edits and deletions reaching
// the file, and the errors that must never lose data: a folder that is not there, a wrong
// passphrase, a damaged file, a file of a newer app and a file that is not a sync file.
//
// One app runs per scenario, so the other computer is not driven here: the merge of two
// computers, the conflicts and their copies are tested in Rust (`commands::sync::tests` with
// two machines, `platypus-sync`'s merge tests).
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { setValue } from "../admin-users.mjs";

const NAME = "E2E Sync";
const CONN = "00000000-0000-4000-8000-0000000000e1";
const PROJECT = "00000000-0000-4000-9000-0000000000e1";
const PASSPHRASE = "correct horse battery staple";
const SECRET_COMMAND = "SECRET-CANARY-7";
const SCRIPT_TEXT = "SELECT 'CANARY-SCRIPT-TEXT' AS v";

const panel = `document.querySelector('.sync-panel')`;
const field = (name) => `${panel}?.querySelector('[data-field="${name}"]')`;
const action = (name) => `${panel}?.querySelector('[data-action="${name}"]')`;
const message = `(${field("sync-message")}?.textContent ?? '')`;
const cloud = (dir) => join(dir, "cloud");
const syncFile = (dir) => join(cloud(dir), "platypus-sync.bin");
const stateFile = (dir) => join(dir, "sync-state.json");
const script = (dir) => join(dir, "scripts", "all", "report.sql");

export function setup({ dir }) {
  writeFileSync(join(dir, "e2e.db"), "");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[projects]]
id = "${PROJECT}"
name = "Team data"
color = "#3d7bf7"
connections = ["${CONN}"]

[[connections]]
id = "${CONN}"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
password_command = "echo ${SECRET_COMMAND}"
`,
  );
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[editor]\nvim = true\n\n[drivers]\nauto_update = false\n`);
  mkdirSync(join(dir, "scripts", "all"), { recursive: true });
  writeFileSync(script(dir), SCRIPT_TEXT);
  mkdirSync(cloud(dir), { recursive: true });
}

/** The last sync the app remembers, in ms; `0` before the first. */
function lastSync(dir) {
  try {
    return JSON.parse(readFileSync(stateFile(dir), "utf8")).last_sync ?? 0;
  } catch {
    return 0;
  }
}

/** The items the app remembers of the last sync, by key. */
function remembered(dir) {
  return JSON.parse(readFileSync(stateFile(dir), "utf8")).base;
}

/** Clicks Sync now and waits for the run to finish: the time of the last sync moves on. A
 *  message that is the same as the one before cannot tell, so the state file does. */
async function syncNow(page, dir) {
  // A click on a button that is still disabled (the sync at start, or the page loading its
  // status) does nothing, so wait for it first.
  await page.waitFor(`${action("sync-now")} && !${action("sync-now")}.disabled`);
  const before = lastSync(dir);
  await page.eval(`${action("sync-now")}.click()`);
  const until = Date.now() + 20_000;
  while (lastSync(dir) <= before) {
    assert.ok(Date.now() < until, `the sync did not finish: ${await page.eval(message)}`);
    await page.sleep(100);
  }
  await page.waitFor(`${action("sync-now")} && !${action("sync-now")}.disabled`);
}

/** Clicks Sync now where it must fail with `pattern`: the message is new, as the one before
 *  was a success or another error. */
async function syncFails(page, pattern) {
  await page.eval(`${action("sync-now")}.click()`);
  await page.waitFor(`${field("sync-message")} !== null && ${field("sync-message")}.classList.contains('error') && ${pattern}.test(${message})`, 20_000);
  await page.waitFor(`!${action("sync-now")}.disabled`);
}

async function openSyncPage(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Sync");
  await page.waitFor(`${panel} !== null`);
}

export default async function (page, { dir }) {
  await openSyncPage(page);
  assert.equal(await page.eval(`${panel}.dataset.sync`), "off");
  assert.match(await page.eval(`${panel}.textContent`), /Not synced yet\./);
  // Nothing to sync with until a folder and a passphrase are given.
  assert.equal(await page.eval(`${action("sync-now")}.disabled`), true);
  assert.equal(await page.eval(`${action("sync-save")}.disabled`), true);

  // A folder that is not there is refused, and nothing is created in its place.
  const missing = join(dir, "not mounted");
  await setValue(page, `.sync-panel [data-field="sync-folder"]`, missing);
  await setValue(page, `.sync-panel [data-field="sync-passphrase"]`, PASSPHRASE);
  await setValue(page, `.sync-panel [data-field="sync-confirm"]`, PASSPHRASE);
  await page.waitFor(`!${action("sync-save")}.disabled`);
  await page.eval(`${action("sync-save")}.click()`);
  await page.waitFor(`/sync folder cannot be used/.test(${message})`);
  assert.equal(existsSync(missing), false);
  assert.equal(await page.eval(`${panel}.dataset.sync`), "off");

  // Two passphrases that differ do not do.
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(cloud(dir))})`);
  await page.eval(`${action("sync-browse")}.click()`);
  await page.waitFor(`${field("sync-folder")}.value === ${JSON.stringify(cloud(dir))}`);
  await setValue(page, `.sync-panel [data-field="sync-confirm"]`, "something else");
  await page.waitFor(`${action("sync-save")}.disabled && /do not match/.test(${panel}.textContent)`);
  await setValue(page, `.sync-panel [data-field="sync-confirm"]`, PASSPHRASE);
  await page.waitFor(`!${action("sync-save")}.disabled`);

  // Turned on: the passphrase is kept (in the keychain), the fields are emptied.
  await page.eval(`${action("sync-save")}.click()`);
  await page.waitFor(`${panel}.dataset.sync === 'on'`);
  assert.equal(await page.eval(`${field("sync-passphrase")}.value`), "");
  assert.equal(existsSync(syncFile(dir)), false, "nothing is written before the first sync");
  const settings = readFileSync(join(dir, "settings.toml"), "utf8");
  assert.match(settings, /\[sync\]/);
  assert.doesNotMatch(settings, new RegExp(PASSPHRASE), "the passphrase is not in a settings file");

  // The first sync writes the file, and nothing in it is readable.
  await syncNow(page, dir);
  assert.equal(existsSync(syncFile(dir)), true);
  const first = readFileSync(syncFile(dir));
  assert.equal(first.subarray(0, 8).toString(), "PLATYSYN");
  const text = first.toString("latin1");
  for (const leak of [SECRET_COMMAND, NAME, "Team data", "CANARY-SCRIPT-TEXT", "report.sql", "editor", "sqlite", "password_command"]) {
    assert.equal(text.includes(leak), false, `${leak} is readable in the sync file`);
  }
  assert.doesNotMatch(await page.eval(`${panel}.textContent`), /Not synced yet\./);

  // What the app remembers: the connection, the project, the script, the setting, and not
  // the machine's own drivers setting.
  const keys = Object.keys(remembered(dir));
  for (const key of [`connection/${CONN}`, `project/${PROJECT}`, "script/all/report.sql", "setting/editor.vim"]) {
    assert.ok(keys.includes(key), `${key} in ${keys.join(", ")}`);
  }
  assert.equal(keys.some((k) => k.startsWith("setting/drivers") || k.startsWith("setting/sync")), false, keys.join(", "));
  assert.doesNotMatch(readFileSync(stateFile(dir), "utf8"), new RegExp(SECRET_COMMAND));

  // Nothing changed: another sync leaves the file as it is.
  await syncNow(page, dir);
  assert.deepEqual(readFileSync(syncFile(dir)), first);

  // An edit of a script reaches the file.
  writeFileSync(script(dir), `${SCRIPT_TEXT} -- edited`);
  await syncNow(page, dir);
  const edited = readFileSync(syncFile(dir));
  assert.notDeepEqual(edited, first);

  // The sync at start: a script written while the app runs is picked up by the sync the
  // reload starts, without a click.
  writeFileSync(join(dir, "scripts", "all", "second.sql"), "SELECT 2");
  const beforeReload = lastSync(dir);
  await page.sleep(600);
  await page.eval(`location.reload()`);
  const started = Date.now() + 30_000;
  while (lastSync(dir) <= beforeReload) {
    assert.ok(Date.now() < started, "the sync at start did not run");
    await page.sleep(200);
  }
  assert.ok(Object.keys(remembered(dir)).includes("script/all/second.sql"));
  const second = readFileSync(syncFile(dir));
  assert.notDeepEqual(second, edited);

  // A deletion is remembered as one, so it travels instead of the script coming back.
  await openSyncPage(page);
  rmSync(join(dir, "scripts", "all", "second.sql"));
  await syncNow(page, dir);
  assert.equal(remembered(dir)["script/all/second.sql"].deleted, true);
  assert.equal(existsSync(join(dir, "scripts", "all", "second.sql")), false);
  const good = readFileSync(syncFile(dir));

  // A damaged file is an error and nothing is changed or lost.
  const damaged = Buffer.from(good);
  damaged[damaged.length - 1] ^= 0x40;
  writeFileSync(syncFile(dir), damaged);
  const stamp = lastSync(dir);
  await syncFails(page, "/Wrong sync passphrase, or the sync file is damaged/");
  assert.deepEqual(readFileSync(syncFile(dir)), damaged, "a file that does not read is not overwritten");
  assert.equal(lastSync(dir), stamp);
  assert.equal(readFileSync(script(dir), "utf8"), `${SCRIPT_TEXT} -- edited`);

  // A file of a newer app is refused with a clear message.
  const newer = Buffer.from(good);
  newer.writeUInt32BE(2, 8);
  writeFileSync(syncFile(dir), newer);
  await syncFails(page, "/newer PlatypusDB/");
  assert.deepEqual(readFileSync(syncFile(dir)), newer);

  // So is a file that is not a sync file at all.
  writeFileSync(syncFile(dir), "just some text");
  await syncFails(page, "/not a PlatypusDB sync file/");
  assert.equal(readFileSync(syncFile(dir), "utf8"), "just some text");

  // The good file again: the next sync works, as if nothing had happened.
  writeFileSync(syncFile(dir), good);
  await syncNow(page, dir);
  assert.equal(await page.eval(`${field("sync-message")}.classList.contains('error')`), false);

  // Another passphrase cannot be switched to over a file the old one made.
  await setValue(page, `.sync-panel [data-field="sync-passphrase"]`, "a different passphrase");
  await setValue(page, `.sync-panel [data-field="sync-confirm"]`, "a different passphrase");
  await page.waitFor(`!${action("sync-save")}.disabled`);
  await page.eval(`${action("sync-save")}.click()`);
  await page.waitFor(`${field("sync-message")}?.classList.contains('error') && /Wrong sync passphrase/.test(${message})`);
  assert.equal(await page.eval(`${panel}.dataset.sync`), "on");
  await syncNow(page, dir);

  // Turned off: the folder, the passphrase and what was remembered are forgotten here; the
  // file in the folder stays for the other computers.
  await page.eval(`${action("sync-off")}.click()`);
  await page.waitFor(`${panel}.dataset.sync === 'off'`);
  assert.equal(existsSync(stateFile(dir)), false);
  assert.equal(existsSync(syncFile(dir)), true);
  assert.equal(await page.eval(`${action("sync-now")}.disabled`), true);
  assert.doesNotMatch(readFileSync(join(dir, "settings.toml"), "utf8"), /folder =/);
}
