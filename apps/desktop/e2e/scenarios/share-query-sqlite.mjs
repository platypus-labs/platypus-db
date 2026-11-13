// Group O: share a query as a link or a `.platypus` file. The Share dialog (from the palette
// and the tab's menu) gives a link and a file with the SQL, a title, the target as a
// descriptor without credentials, and the variables' values only when asked; opening a file
// (the palette's picker, a drop, the command line: all one path), or a pasted link, finds the
// saved connection (the only exact one at once, else a choice among the exact and the likely
// ones, else a new connection started from the descriptor) and opens the SQL in a tab that
// is never run, with a notice asking for a review. Bad files and links are refused.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { button, fill, formField, saveForm } from "../conn-helpers.mjs";
import { palette } from "../plans.mjs";
import { editorText, newEditor, saveConnections, sqliteFile, status, visibleTab } from "../analysis-helpers.mjs";
import { SHARE_VERSION, decodeFile, decodeLink, encodeFile, encodeLink } from "../../src/share/shareQuery.ts";

const MAIN = "Share main";
const COPY = "Share main copy";
const OTHER = "Share other";
const ids = {
  main: "00000000-0000-4000-8000-0000000b0001",
  copy: "00000000-0000-4000-8000-0000000b0002",
  other: "00000000-0000-4000-8000-0000000b0003",
};
const SQL = "SELECT id, name FROM items WHERE id > :min ORDER BY id";

const share = `document.querySelector('.share-dialog')`;
const linkBox = `(document.querySelector('.share-dialog [data-field="share_link"]')?.value ?? '')`;
const shareMessage = `(document.querySelector('.share-dialog [data-field="share_message"]')?.textContent ?? '')`;
const openDialog = `document.querySelector('.modal.share-open')`;
const linkDialog = `document.querySelector('.share-link-dialog')`;
const sharedNotice = `${visibleTab}?.querySelector('[data-field="shared_notice"]')`;
const toast = `(document.querySelector('.toast')?.textContent ?? '')`;
const tabCount = `document.querySelectorAll('.query-tab').length`;
const activeTab = `(document.querySelector('.tab.active')?.textContent ?? '')`;
const rows = (group) => `[...document.querySelectorAll('.modal.share-open [data-group=${JSON.stringify(group)}] .share-choice')].map(r => r.dataset.id)`;

let files;

export function setup({ dir }) {
  const main = sqliteFile(dir, "main.db", "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES (1, 'a'), (2, 'b'), (3, 'c');");
  const other = sqliteFile(dir, "other.db", "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES (1, 'x'), (2, 'y'), (3, 'z');");
  saveConnections(dir, [
    { id: ids.main, name: MAIN, driver: "sqlite", database: main },
    { id: ids.copy, name: COPY, driver: "sqlite", database: main },
    { id: ids.other, name: OTHER, driver: "sqlite", database: other },
  ]);
  files = { main, other };
}

/** Replaces the main editor's text and waits for it to show. */
async function setSql(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.editor .cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
}

const target = (over = {}) => ({ driver: "sqlite", host: "", port: null, database: files.other, schema: null, name: OTHER, ...over });
const shared = (over = {}) => ({ title: "Items", sql: SQL, parameters: {}, target: target(), ...over });

/** Writes a `.platypus` file of `query` in `dir`. */
function writeShared(dir, name, query) {
  const path = join(dir, name);
  writeFileSync(path, encodeFile(query));
  return path;
}

/** Opens what the OS would hand the app: a path or a link. */
const openShared = (page, item) => page.eval(`window.__platypus.openShared(${JSON.stringify(item)})`);

/** A link built by hand, for what the app never writes. */
const rawLink = (doc) => `platypus://query?d=${Buffer.concat([Buffer.from([0]), Buffer.from(JSON.stringify(doc))]).toString("base64url")}`;

async function closeDialog(page, root, label = "Close") {
  await page.eval(`${button(root, label)}.click()`);
  await page.waitFor(`${root} === null`);
}

/** Opens the Share dialog of the visible editor from the palette. */
async function openShare(page) {
  await palette(page, "Share query");
  await page.waitFor(`${share} !== null`);
}

/** Sets the Share dialog's title as typing would. */
async function setTitle(page, title) {
  await page.eval(fill(`document.querySelector('.share-dialog input[aria-label="Title"]')`, title));
}

/** Waits for the link box to show something other than `before`, and returns it. */
async function newLink(page, before) {
  await page.waitFor(`${linkBox} !== '' && ${linkBox} !== ${JSON.stringify(before)}`);
  return page.eval(linkBox);
}

/** Opens a tab on a shared file the way the palette does, with the picker answered. */
async function openFileFromPalette(page, path) {
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(path)})`);
  await palette(page, "Open a shared query");
}

/** Waits for a new editor tab and returns its text; the query must not have run. */
async function waitNewTab(page, before) {
  await page.waitFor(`${tabCount} > ${before} && ${visibleTab}?.querySelector('.editor .cm-content') != null`, 15_000);
}

/** A shared query opened is shown, with its notice, and nothing ran. */
async function assertNotRun(page) {
  await page.waitFor(`${sharedNotice} != null`);
  assert.equal(await page.eval(`${sharedNotice}.textContent.startsWith('Shared query: review before running.')`), true);
  // A run would show its rows or an error within moments.
  await page.sleep(600);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.grid-scroll') === null`), true);
  assert.equal(await page.eval(status), "");
}

export default async function (page, { dir }) {
  // ---- Sharing ----
  await newEditor(page, OTHER);
  await setSql(page, SQL);
  // The variable the query needs, with a value, in the Variables panel.
  await page.eval(`${button(visibleTab, "Variables")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.variables-panel input[aria-label="New variable"]') != null`);
  await page.eval(fill(`${visibleTab}.querySelector('.variables-panel input[aria-label="New variable"]')`, "min"));
  await page.eval(`${visibleTab}.querySelector('.variables-panel .variable-add button[type=submit]').click()`);
  await page.waitFor(`${visibleTab}.querySelector('.variables-panel input[aria-label="Value of min"]') != null`);
  await page.eval(fill(`${visibleTab}.querySelector('.variables-panel input[aria-label="Value of min"]')`, "1"));
  await page.waitFor(`${visibleTab}.querySelector('.variables-panel input[aria-label="Value of min"]').value === '1'`);

  await openShare(page);
  // The target is described, the secrets are said not to travel, and the values are off.
  assert.ok((await page.eval(`document.querySelector('.share-dialog [data-field="share_target"]').textContent`)).includes(`${OTHER} (sqlite)`));
  assert.equal(await page.eval(`document.querySelector('.share-dialog input[type=checkbox]').checked`), false);
  await page.waitFor(`${linkBox}.startsWith('platypus://query?d=')`);
  await setTitle(page, "Items above");
  const first = await newLink(page, "");
  const decoded = await decodeLink(first);
  assert.equal(decoded.title, "Items above");
  assert.equal(decoded.sql, SQL);
  assert.deepEqual(decoded.parameters, {});
  assert.deepEqual(decoded.target, target());

  // The values go in only when chosen, with a warning; unchecking takes them out again.
  await page.eval(`document.querySelector('.share-dialog input[type=checkbox]').click()`);
  const withValues = await newLink(page, first);
  assert.deepEqual((await decodeLink(withValues)).parameters, { min: "1" });
  assert.equal(await page.eval(`document.querySelector('.share-dialog .notice.warn') !== null`), true);
  await page.eval(`document.querySelector('.share-dialog input[type=checkbox]').click()`);
  assert.equal(await newLink(page, withValues), first);

  // Copy link puts it on the clipboard: the open-link box pastes the same text back.
  await page.eval(`${button(share, "Copy link")}.click()`);
  await page.waitFor(`${button(share, "Copied")} != null`);
  await closeDialog(page, share);
  await palette(page, "Open a shared link");
  await page.waitFor(`${linkDialog} !== null`);
  assert.equal(await page.eval(`${button(linkDialog, "Open")}.disabled`), true);
  await page.eval(`${button(linkDialog, "Paste")}.click()`);
  await page.waitFor(`${linkDialog}.querySelector('textarea').value === ${JSON.stringify(first)}`);
  await closeDialog(page, linkDialog, "Cancel");

  // Save as .platypus (the picker answered): the file has the layout, the values only when
  // chosen, and no credentials anywhere.
  const saved = join(dir, "items.platypus");
  await openShare(page);
  await setTitle(page, "Items above");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(saved)})`);
  await page.eval(`${button(share, "Save as .platypus…")}.click()`);
  await page.waitFor(`${shareMessage}.includes('items.platypus')`);
  const doc = JSON.parse(readFileSync(saved, "utf8"));
  assert.equal(doc.format, "platypus-query");
  assert.equal(doc.version, SHARE_VERSION);
  assert.equal("parameters" in doc, false);
  assert.deepEqual(Object.keys(doc.target).sort(), ["database", "driver", "host", "name", "port", "schema"]);
  assert.deepEqual(decodeFile(readFileSync(saved, "utf8")).target, target());
  await page.eval(`document.querySelector('.share-dialog input[type=checkbox]').click()`);
  await page.waitFor(`document.querySelector('.share-dialog .notice.warn') !== null`);
  const withParams = join(dir, "items-values.platypus");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(withParams)})`);
  await page.eval(`${button(share, "Save as .platypus…")}.click()`);
  await page.waitFor(`${shareMessage}.includes('items-values.platypus')`);
  assert.deepEqual(JSON.parse(readFileSync(withParams, "utf8")).parameters, { min: "1" });
  await closeDialog(page, share);

  // The tab's own menu has Share query… too.
  const tab = await page.rect(".tab.active");
  await page.click(tab.x + 20, tab.y + tab.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith('Share query')).click()`);
  await page.waitFor(`${share} !== null`);
  await closeDialog(page, share);

  // An empty editor has nothing to share.
  await newEditor(page, MAIN);
  await openShare(page);
  assert.equal(await page.eval(`${share}.querySelector('.notice.warn') !== null`), true);
  assert.equal(await page.eval(`${button(share, "Copy link")}.disabled && ${button(share, "Save as .platypus…")}.disabled`), true);
  assert.equal(await page.eval(`${share}.querySelector('[data-field="share_link"]')`), null);
  await closeDialog(page, share);

  // A query too long for a link: the link is refused with a hint, the file still saves.
  let seed = 11;
  const noise = Array.from({ length: 16000 }, () => String.fromCharCode(97 + (((seed = (seed * 1103515245 + 12345) & 0x7fffffff) >> 8) % 26))).join("");
  const huge = `SELECT '${noise}' AS big`;
  let before = await page.eval(tabCount);
  await openShared(page, writeShared(dir, "huge.platypus", shared({ title: "Huge", sql: huge })));
  await waitNewTab(page, before);
  await openShare(page);
  await page.waitFor(`document.querySelector('.share-dialog [data-field="share_link_note"]')?.textContent.includes('too long for a link')`);
  assert.equal(await page.eval(`${button(share, "Copy link")}.disabled`), true);
  assert.equal(await page.eval(`${button(share, "Save as .platypus…")}.disabled`), false);
  await closeDialog(page, share);

  // ---- Opening a file: the only exact connection opens at once ----
  before = await page.eval(tabCount);
  await openFileFromPalette(page, withParams);
  await waitNewTab(page, before);
  assert.equal(await page.eval(editorText), SQL);
  assert.ok((await page.eval(activeTab)).includes(OTHER), await page.eval(activeTab));
  assert.ok((await page.eval(activeTab)).includes("Items above"));
  await assertNotRun(page);
  // The values came with it.
  await page.eval(`${button(visibleTab, "Variables")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.variables-panel input[aria-label="Value of min"]')?.value === '1'`);
  // The notice goes away when dismissed.
  await page.eval(`${button(sharedNotice, "Dismiss")}.click()`);
  await page.waitFor(`${sharedNotice} === null`);

  // A file without values opens without any variable.
  before = await page.eval(tabCount);
  await openFileFromPalette(page, saved);
  await waitNewTab(page, before);
  await assertNotRun(page);
  await page.eval(`${button(visibleTab, "Variables")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.variables-panel .history-empty') != null`);

  // A drop of the file on the window is the same.
  before = await page.eval(tabCount);
  await page.eval(`window.__platypus.openDroppedFile(${JSON.stringify(saved)})`);
  await waitNewTab(page, before);
  await assertNotRun(page);

  // ---- Several exact connections: the user picks ----
  const twice = writeShared(dir, "main.platypus", shared({ title: "Main items", target: target({ database: files.main, name: MAIN }) }));
  before = await page.eval(tabCount);
  await openShared(page, twice);
  await page.waitFor(`${openDialog} !== null`);
  assert.deepEqual(await page.eval(rows("exact")), [ids.main, ids.copy]);
  assert.deepEqual(await page.eval(rows("candidates")), [ids.other]);
  assert.ok((await page.eval(`document.querySelector('.modal.share-open [data-field="share_sql"]').textContent`)).includes(SQL));
  assert.equal(await page.eval(`${openDialog}.querySelector('h2').textContent`), "Main items");
  // The first exact one is chosen; take the second.
  await page.eval(`document.querySelector('.share-choice[data-id="${ids.copy}"] input').click()`);
  await page.waitFor(`document.querySelector('.share-choice[data-id="${ids.copy}"] input').checked`);
  await page.eval(`${button(openDialog, "Open")}.click()`);
  await waitNewTab(page, before);
  assert.ok((await page.eval(activeTab)).includes(COPY), await page.eval(activeTab));
  assert.equal(await page.eval(editorText), SQL);
  await assertNotRun(page);

  // Cancel opens nothing.
  before = await page.eval(tabCount);
  await openShared(page, twice);
  await page.waitFor(`${openDialog} !== null`);
  await closeDialog(page, openDialog, "Cancel");
  await page.sleep(300);
  assert.equal(await page.eval(tabCount), before);

  // ---- A database that is not saved: likely connections of the driver ----
  const elsewhere = writeShared(dir, "elsewhere.platypus", shared({ title: "Elsewhere", target: target({ database: join(dir, "nowhere.db"), name: OTHER }) }));
  before = await page.eval(tabCount);
  await openShared(page, elsewhere);
  await page.waitFor(`${openDialog} !== null`);
  assert.equal(await page.eval(`document.querySelector('.modal.share-open [data-group="exact"]')`), null);
  // The one with the sender's name first, none chosen yet.
  assert.deepEqual(await page.eval(rows("candidates")), [ids.other, ids.main, ids.copy]);
  assert.equal(await page.eval(`${openDialog}.querySelector('input:checked')`), null);
  assert.equal(await page.eval(`${button(openDialog, "Open")}.disabled`), true);
  await page.eval(`document.querySelector('.share-choice[data-id="${ids.main}"] input').click()`);
  await page.waitFor(`!${button(openDialog, "Open")}.disabled`);
  await page.eval(`${button(openDialog, "Open")}.click()`);
  await waitNewTab(page, before);
  assert.ok((await page.eval(activeTab)).includes(MAIN), await page.eval(activeTab));
  await assertNotRun(page);

  // ---- A database of a kind not saved: a new connection from the descriptor ----
  const remote = writeShared(
    dir,
    "remote.platypus",
    shared({
      title: "Orders",
      sql: "SELECT 1",
      target: { driver: "postgres", host: "db.example.com", port: 5433, database: "shop", schema: "public", name: "Shop DB" },
    }),
  );
  before = await page.eval(tabCount);
  await openShared(page, remote);
  await page.waitFor(`${openDialog} !== null`);
  assert.equal(await page.eval(`${openDialog}.querySelector('.share-choice')`), null);
  assert.equal(await page.eval(`${button(openDialog, "Open")}.disabled`), true);
  await page.eval(`${button(openDialog, "New connection from this…")}.click()`);
  await page.waitFor(`document.querySelector('.connection-dialog') !== null`);
  const value = (label) => page.eval(`${formField(label)}.value`);
  assert.equal(await value("Name"), "Shop DB");
  assert.equal(await value("Host"), "db.example.com");
  assert.equal(await value("Port"), "5433");
  assert.equal(await value("User"), "");
  // Nothing was opened or run yet.
  assert.equal(await page.eval(tabCount), before);
  // Saving the connection opens the query on it, still not run.
  await saveForm(page);
  await waitNewTab(page, before);
  assert.equal(await page.eval(editorText), "SELECT 1");
  assert.ok((await page.eval(activeTab)).includes("Shop DB"), await page.eval(activeTab));
  await assertNotRun(page);

  // ---- Links pasted in "Open a shared link…" ----
  const link = await encodeLink(shared({ title: "From a link", parameters: { min: "2" } }));
  before = await page.eval(tabCount);
  await palette(page, "Open a shared link");
  await page.waitFor(`${linkDialog} !== null`);
  await page.eval(fill(`${linkDialog}.querySelector('textarea')`, `  ${link}\n`));
  await page.waitFor(`!${button(linkDialog, "Open")}.disabled`);
  await page.eval(`${button(linkDialog, "Open")}.click()`);
  await waitNewTab(page, before);
  assert.equal(await page.eval(editorText), SQL);
  assert.ok((await page.eval(activeTab)).includes("From a link"));
  await assertNotRun(page);
  await page.eval(`${button(visibleTab, "Variables")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.variables-panel input[aria-label="Value of min"]')?.value === '2'`);

  // Refusals stay in the box, with the reason, and open nothing.
  before = await page.eval(tabCount);
  await palette(page, "Open a shared link");
  await page.waitFor(`${linkDialog} !== null`);
  const refuse = async (text, message) => {
    await page.eval(fill(`${linkDialog}.querySelector('textarea')`, text));
    await page.waitFor(`!${button(linkDialog, "Open")}.disabled`);
    await page.eval(`${button(linkDialog, "Open")}.click()`);
    await page.waitFor(`(${linkDialog}.querySelector('.notice.error')?.textContent ?? '').includes(${JSON.stringify(message)})`);
  };
  await refuse("hello", "not a valid PlatypusDB query link");
  await refuse("https://example.com/?d=AAAA", "not a valid PlatypusDB query link");
  await refuse("platypus://query?d=!!!", "not a valid PlatypusDB query link");
  await refuse(rawLink({ format: "something-else" }), "not a PlatypusDB shared query");
  await refuse(rawLink({ format: "platypus-query", version: SHARE_VERSION + 1, sql: "SELECT 1", target: target() }), "does not read");
  await refuse(rawLink({ format: "platypus-query", version: SHARE_VERSION, sql: 5, target: target() }), "damaged");
  await refuse(JSON.stringify({ format: "platypus-query", version: SHARE_VERSION, sql: "SELECT 2", target: "nope" }), "damaged");
  // Typing in the box clears the error.
  await page.eval(fill(`${linkDialog}.querySelector('textarea')`, "x"));
  await page.waitFor(`${linkDialog}.querySelector('.notice.error') === null`);
  await closeDialog(page, linkDialog, "Cancel");
  assert.equal(await page.eval(tabCount), before);

  // ---- Bad files: the error is shown and nothing opens ----
  const bad = async (name, content, message) => {
    const path = join(dir, name);
    writeFileSync(path, content);
    await page.eval(`document.querySelector('.toast')?.click()`);
    await page.waitFor(`document.querySelector('.toast') === null`);
    await openShared(page, path);
    await page.waitFor(`${toast}.includes(${JSON.stringify(message)})`);
    assert.equal(await page.eval(tabCount), before);
  };
  await bad("notjson.platypus", "this is not json", "not a PlatypusDB shared query");
  await bad("array.platypus", "[]", "not a PlatypusDB shared query");
  await bad("future.platypus", JSON.stringify({ format: "platypus-query", version: 99, sql: "SELECT 1", target: target() }), "does not read");
  await bad("shape.platypus", JSON.stringify({ format: "platypus-query", version: SHARE_VERSION, sql: "SELECT 1", target: { driver: 7 } }), "damaged");
  await bad("huge-field.platypus", JSON.stringify({ ...JSON.parse(encodeFile(shared())), title: "t".repeat(2000) }), "too large");
  // A file that is not there.
  await page.eval(`document.querySelector('.toast')?.click()`);
  await page.waitFor(`document.querySelector('.toast') === null`);
  await openShared(page, join(dir, "missing.platypus"));
  await page.waitFor(`${toast}.includes('missing.platypus')`);
  assert.equal(await page.eval(tabCount), before);
}
