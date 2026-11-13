// What the three complete user journeys (journeys/first-start, import-dbeaver, workday) share
// beyond journey.mjs: one `j` object (page, mode, `t`, `kb` for the keyboard-only run) and
// steps that behave by the run's mode. With the mouse (en, it) a step clicks or fills; with
// the keyboard only (keys) it reaches the same control with Tab, the arrows, Enter, Escape,
// the shortcuts and the command palette, and checks the focus is visible there.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { crc32 } from "node:zlib";
import { ALT, CTRL } from "./cdp.mjs";
import { fill } from "./conn-helpers.mjs";
import { focusVisible, keys, screenIsItalian, space, tabStep, texts } from "./journey.mjs";
import { reopen, restart, status, visibleTab } from "./journey-helpers.mjs";

export { focusVisible, keys, screenIsItalian, status, visibleTab };

export const journey = (page, mode) => ({ page, mode, kb: mode === "keys", t: texts(mode) });

/** The text of the run's language for `english`, `{name}` placeholders filled from `vars`. */
export const fmt = (j, english, vars = {}) => j.t(english).replace(/\{(\w+)\}/g, (_, k) => String(vars[k]));

/** "3 rows" / "1 row" as the status bar says it. */
export const rowsText = (j, n) => fmt(j, n === 1 ? "{n} row" : "{n} rows", { n });

/** `el` is the active element in `test`. Tab (Shift+Tab when `back`) until it holds; the focus
 *  must show a ring there. (An element is never returned by value: a React node is too deep.) */
export async function tabTo(j, test, max = 60, { back = false, ring = true } = {}) {
  const holds = `(() => { const el = document.activeElement; return !!el && el !== document.body && !!(${test}); })()`;
  for (let i = 0; i <= max; i++) {
    if (await j.page.eval(holds)) return ring ? assert.equal(await j.page.eval(focusVisible), true, `the focus is not visible on ${test}`) : undefined;
    await tabStep(j.page, { back });
  }
  throw new Error(`Tab never reached ${test}`);
}

/** Types `text` as key presses (one key event with its character each), as a typist does. */
export async function typeKeys(page, text) {
  for (const ch of text) {
    const code = /[a-z]/i.test(ch) ? `Key${ch.toUpperCase()}` : /\d/.test(ch) ? `Digit${ch}` : "Unidentified";
    await page.send("Input.dispatchKeyEvent", { type: "keyDown", key: ch, code, text: ch, unmodifiedText: ch, windowsVirtualKeyCode: ch.toUpperCase().charCodeAt(0) });
    await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: ch, code, windowsVirtualKeyCode: ch.toUpperCase().charCodeAt(0) });
  }
}

export const ctrl = (page, key, code, keyCode) => page.key(key, code, keyCode, CTRL);

/** The label text of the focused control's field, to find a form field by Tab. */
const fieldLabel = `(el.closest('label')?.querySelector('span')?.textContent ?? '')`;
const isButton = (text) => `el.tagName === 'BUTTON' && (el.textContent.trim() === ${JSON.stringify(text)} || el.getAttribute('aria-label') === ${JSON.stringify(text)} || el.title === ${JSON.stringify(text)})`;

/** A button of `scope` (a page expression) by its text. Mouse: click; keyboard: Tab, Enter. */
export async function pressButton(j, scope, english, vars = undefined) {
  const text = vars ? fmt(j, english, vars) : j.t(english);
  if (!j.kb) {
    await j.page.eval(`(() => { const b = [...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)} || b.getAttribute('aria-label') === ${JSON.stringify(text)} || b.title === ${JSON.stringify(text)}); if (!b) throw new Error('no button ${text}'); b.click(); })()`);
    return;
  }
  await tabTo(j, isButton(text));
  await keys.enter(j.page);
}

/** Sets the form field labelled `english` of `scope` (a CSS selector of the dialog) to `value`. */
export async function setField(j, scope, english, value) {
  const label = j.t(english);
  const field = `[...document.querySelectorAll(${JSON.stringify(`${scope} label.field`)})].find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)})?.querySelector('input, select, textarea')`;
  if (!j.kb) {
    await j.page.waitFor(`${field} != null`);
    await j.page.eval(fill(field, String(value)));
  } else {
    // A field before the focus (the password, retyped after a test) is reached backwards.
    const back = await j.page.eval(`(() => { const f = ${field}; return !!f && !!document.activeElement && (f.compareDocumentPosition(document.activeElement) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0; })()`);
    await tabTo(j, `${fieldLabel} === ${JSON.stringify(label)}`, 60, { back });
    await ctrl(j.page, "a", "KeyA", 65);
    await j.page.type(String(value));
  }
  await j.page.waitFor(`${field}.value === ${JSON.stringify(String(value))}`);
}

/** Sets the `<select>` labelled `english` to the option of value `value`. Keyboard: Tab to it,
 *  then the arrows until the option is chosen. */
export function setSelect(j, scope, english, value) {
  const label = j.t(english);
  return selectEl(j, `[...document.querySelectorAll(${JSON.stringify(`${scope} label`)})].find(l => l.querySelector('span')?.textContent?.includes(${JSON.stringify(label)}))?.querySelector('select')`, value);
}

/** Sets the `<select>` the page expression `field` finds to `value`. */
export async function selectEl(j, field, value) {
  await j.page.waitFor(`${field} != null`);
  if (!j.kb) return j.page.eval(fill(field, value));
  await tabTo(j, `el === ${field}`);
  for (let i = 0; i < 40 && (await j.page.eval(`${field}.value`)) !== value; i++) {
    await keys.down(j.page);
    await j.page.sleep(60);
  }
  assert.equal(await j.page.eval(`${field}.value`), value);
}

/** The command palette: Ctrl+K, type `text`, move to the result whose label contains `pick`,
 *  Enter. Always the keyboard: that is how the palette is used. */
export async function palette(j, text, pick = text) {
  const { page } = j;
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') === document.activeElement`, 5_000);
  await page.type(text);
  const labels = `[...document.querySelectorAll('.palette [role=option]')].map(o => o.querySelector('.palette-label')?.textContent ?? '')`;
  await page.waitFor(`${labels}.some(l => l.includes(${JSON.stringify(pick)}))`, 10_000);
  // The palette keeps its focus in the search field; the chosen result is what shows it.
  assert.equal(await page.eval(`document.querySelector(".palette li.selected") !== null`), true, "the palette shows no chosen result");
  for (let i = 0; i < 60; i++) {
    const at = await page.eval(`(() => { const l = ${labels}; const s = [...document.querySelectorAll('.palette [role=option]')].findIndex(o => o.getAttribute('aria-selected') === 'true'); return l[s]?.includes(${JSON.stringify(pick)}) ?? false; })()`);
    if (at) break;
    await keys.down(page);
    await page.sleep(40);
  }
  await keys.enter(page);
  await page.waitFor(`document.querySelector('.palette') === null`, 5_000);
}

/** Reaches the tree row labelled `labels.at(-1)` through the rows `labels` (each below the one
 *  before), expanding them, and opens it (Enter, or a double click) when `open`. */
export async function treeOpen(j, labels, open = false) {
  const { page } = j;
  const last = labels.at(-1);
  if (!j.kb) {
    const row = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
    for (const [i, label] of labels.entries()) {
      const chain = labels.slice(0, i);
      const find = `(() => { const rows = [...document.querySelectorAll('.tree-row')]; let at = -1; for (const w of ${JSON.stringify(chain)}) { at = rows.findIndex((r, k) => k > at && r.querySelector('.tree-label')?.textContent === w); if (at < 0) return undefined; } return rows.find((r, k) => k > at && r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)}); })()`;
      await page.waitFor(`${find} !== undefined`, 30_000);
      if (i < labels.length - 1) await page.eval(`${find}.getAttribute('aria-expanded') !== 'true' && ${find}.querySelector('.twisty').click()`);
      else if (open) {
        await page.eval(`${find}.id = 'jrn-open'`);
        await page.clickSel("#jrn-open", { clickCount: 2 });
        await page.eval(`document.getElementById('jrn-open')?.removeAttribute('id')`);
      }
    }
    return;
  }
  const label = `(document.activeElement.querySelector?.('.tree-label')?.textContent)`;
  if (!(await page.eval(`document.activeElement.classList.contains('tree-row')`))) await tabTo(j, `el.classList.contains('tree-row')`, 80);
  await page.key("Home", "Home", 36);
  for (const [i, want] of labels.entries()) {
    const deadline = Date.now() + 40_000;
    for (let seen = await page.eval(label); seen !== want; seen = await page.eval(label)) {
      assert.ok(Date.now() < deadline, `the row ${want} never came under the arrows (on ${seen})`);
      // The tree draws again as a connection connects and may drop the focus: Tab brings it back
      // to the row the tree remembers.
      if (seen === undefined) {
        await tabTo(j, `el.classList.contains('tree-row')`, 80);
        continue;
      }
      await page.eval(`(window.__rowBefore = document.activeElement, 0)`);
      await keys.down(page);
      await page.waitFor(`document.activeElement !== window.__rowBefore`, 600).catch(() => {});
    }
    assert.equal(await page.eval(focusVisible), true, `no visible focus on the row ${want}`);
    if (i < labels.length - 1 && (await page.eval(`document.activeElement.getAttribute('aria-expanded') === 'false'`))) {
      await keys.right(page);
      await page.waitFor(`document.activeElement.getAttribute('aria-expanded') === 'true'`, 30_000);
      // Its children come a moment after it opens (a connection connects first): the next row
      // of the path must be among them before the arrows go down.
      await page.waitFor(
        `(() => { const a = document.activeElement; const level = Number(a.getAttribute('aria-level')); for (let n = a.nextElementSibling; n && Number(n.getAttribute('aria-level')) > level; n = n.nextElementSibling) if (n.querySelector('.tree-label')?.textContent === ${JSON.stringify(labels[i + 1])}) return true; return false; })()`,
        30_000,
      );
    }
  }
  if (open) await keys.enter(page);
  assert.equal(await page.eval(label), last);
}

/** Polls `read()` until it returns `want` (a file or a database changed behind the app). */
export async function until(read, want, what, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  let got = read();
  while (got !== want && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 200));
    got = read();
  }
  assert.equal(got, want, what);
}

/** The picker's tile `label` (a database's name): typed in the search and Tab, Enter; or clicked. */
export async function pickDriver(j, label) {
  const { page } = j;
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
  await page.waitFor(`[...document.querySelectorAll('.driver-tile')].some(b => b.textContent === ${JSON.stringify(label)})`, 15_000);
  await screenIsItalian(page, j.mode, "the driver picker");
  if (!j.kb) return page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
  await page.type(label);
  await tabTo(j, `el.classList.contains('driver-tile')`);
  assert.equal(await page.eval(`document.activeElement.textContent`), label);
  await keys.enter(page);
}

/** Clicks Test connection and returns `{ ok, text }` of the notice that answers. */
export async function testConnection(j) {
  const { page } = j;
  await page.eval(`document.querySelectorAll('.connection-dialog .notice').forEach(n => n.dataset.old = '1')`);
  await pressButton(j, `document.querySelector('.connection-dialog')`, "Test connection");
  const fresh = `document.querySelector('.connection-dialog .notice.ok:not([data-old]), .connection-dialog .notice.error:not([data-old])')`;
  await page.waitFor(`${fresh} !== null`, 40_000);
  return { ok: await page.eval(`${fresh}.classList.contains('ok')`), text: await page.eval(`${fresh}.textContent`) };
}

/** Waits for the status bar of the visible tab to read `n` rows. */
export const waitRows = (j, n, timeout = 20_000) => j.page.waitFor(`${status} === ${JSON.stringify(rowsText(j, n))}`, timeout);

/** Opens the settings (Ctrl+, with the keyboard, the top bar's button otherwise). */
export async function openSettings(j) {
  if (j.kb) await j.page.key(",", "Comma", 188, CTRL);
  else await j.page.eval(`document.querySelector('button[aria-label=${JSON.stringify(j.t("Settings"))}]').click()`);
  await j.page.waitFor(`document.querySelector('.modal.settings') !== null`, 10_000);
}

/** Sets the `<select>` setting `key` of the open Settings to `value`. With the keyboard the
 *  settings are searched first (`search`, typed in the field that has the focus), so that the
 *  select is a few Tabs away. */
export async function setSettingSelect(j, key, search, value) {
  if (j.kb) {
    await j.page.waitFor(`document.activeElement === document.querySelector('.modal.settings input[aria-label]')`, 5_000);
    await j.page.type(j.t(search));
    await j.page.waitFor(`document.querySelector('.modal.settings label[data-key="${key}"]') !== null`, 5_000);
  }
  await selectEl(j, `document.querySelector('.modal.settings label[data-key="${key}"] select')`, value);
}

/** Closes the settings with Escape or the Close button. */
export async function closeSettings(j) {
  if (j.kb) await keys.escape(j.page);
  else await pressButton(j, `document.querySelector('.modal.settings')`, "Done");
  await j.page.waitFor(`document.querySelector('.modal.settings') === null`, 5_000);
}

/** Writes a zip (entries stored, not compressed) of `{ name: text }`: a DBeaver project export. */
export function writeZip(path, entries) {
  const parts = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of Object.entries(entries)) {
    const data = Buffer.from(text);
    const file = Buffer.from(name);
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(file.length, 26);
    parts.push(local, file, data);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(data.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(file.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, file);
    offset += local.length + file.length + data.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(central.length / 2, 8);
  end.writeUInt16LE(central.length / 2, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  writeFileSync(path, Buffer.concat([...parts, directory, end]));
}

/** Replaces the visible tab's SQL with `sql`: the focus goes to the editor (Tab, with the
 *  keyboard) and the text is typed. */
export async function typeSql(j, sql) {
  const { page } = j;
  const editor = `${visibleTab}?.querySelector('.editor .cm-content')`;
  await page.waitFor(`${editor} != null`, 20_000);
  if (!(await page.eval(`document.activeElement === ${editor}`))) {
    if (j.kb) await tabTo(j, `el.classList.contains('cm-content')`, 120);
    else await page.eval(`${editor}.focus()`);
  }
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${editor}.querySelectorAll(".cm-line")].map(l => l.textContent).join("\\n") === ${JSON.stringify(sql)}`);
  if (await page.eval(`document.querySelector('.cm-tooltip-autocomplete') !== null`)) await keys.escape(page);
}

/** Types `sql` and runs it (the statement under the cursor with Ctrl+Enter, the script with
 *  Alt+X). With `rows`, waits for the status to read that many rows. */
export async function runSql(j, sql, { script = false, rows = null } = {}) {
  const { page } = j;
  await typeSql(j, sql);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  if (script) await page.key("x", "KeyX", 88, ALT);
  else await page.key("Enter", "Enter", 13, CTRL);
  if (rows !== null) await waitRows(j, rows, 30_000);
}

/** Presses the button `english` of the dialog that is the last answer before the app ends,
 *  without waiting for the page (it goes away under the call), then waits for the DevTools
 *  port to be free: the app and its web view have gone. */
export async function answerAndExit(j, ctx, scope, english) {
  const text = j.t(english);
  if (j.kb) {
    await tabTo(j, isButton(text));
    keys.enter(j.page).catch(() => {});
  } else {
    j.page.eval(`[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)}).click()`).catch(() => {});
  }
  const deadline = Date.now() + 30_000;
  const open = () => new Promise((done) => {
    const socket = tcp({ host: "127.0.0.1", port: ctx.port, timeout: 500 });
    socket.on("connect", () => (socket.destroy(), done(true)));
    socket.on("error", () => done(false));
    socket.on("timeout", () => (socket.destroy(), done(false)));
  });
  while (Date.now() < deadline && (await open())) await new Promise((r) => setTimeout(r, 300));
  assert.equal(await open(), false, "the app did not end after the answer");
}

/** `reopen` closes the old page by the `close` a first `reopen` gave it, and that ends every app
 *  this run started, the one it has just started too: a second restart in one scenario would
 *  kill the new app. The old page's socket is left to the process end instead. */
export function keepNewApp(page) {
  page.close = () => {};
}

/** `reopen` of journey-helpers.mjs for a second restart in a scenario (see `keepNewApp`). */
export async function reopenApp(page, ctx, ready) {
  keepNewApp(page);
  return reopen(page, ctx, ready);
}

/** `restart` of journey-helpers.mjs, with the page's calls bounded: a call made as the app goes away
 *  (closeApp reads the dialog while it exits) would never answer, and the run would end silently. */
export async function restartApp(page, ctx, options = { ready: `document.querySelector('.tree-row') !== null` }) {
  keepNewApp(page);
  const send = page.send;
  page.send = (method, params) =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`${method}: no answer, the page has gone`)), 8_000);
      send(method, params).then(
        (r) => (clearTimeout(timer), resolve(r)),
        (e) => (clearTimeout(timer), reject(e)),
      );
    });
  return restart(page, ctx, options);
}

/** Ticks or unticks the checkbox of the `label.check` whose text starts with `english`. */
export async function setCheck(j, scope, english, want) {
  const label = j.t(english);
  const box = `[...document.querySelectorAll(${JSON.stringify(`${scope} label`)})].find(l => l.textContent.trim().startsWith(${JSON.stringify(label)}))?.querySelector('input[type=checkbox]')`;
  await j.page.waitFor(`${box} != null`);
  if ((await j.page.eval(`${box}.checked`)) === want) return;
  if (j.kb) {
    await tabTo(j, `el === ${box}`);
    await space(j.page);
  } else await j.page.eval(`${box}.click()`);
  await j.page.waitFor(`${box}.checked === ${want}`);
}

/** The context menu of the tree row reached through `labels`, and the entries `path` chosen in
 *  it: right click and clicks with the mouse; the Menu key, arrows and Enter with the keyboard. */
export async function treeMenu(j, labels, ...path) {
  const { page } = j;
  const last = labels.at(-1);
  if (!j.kb) {
    await treeOpen(j, labels);
    const row = `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(last)}).at(-1)`;
    await page.eval(`document.querySelectorAll('#jrn-row').forEach(e => e.removeAttribute('id')); ${row}.querySelector('.tree-label').id = 'jrn-row'`);
    await page.clickSel("#jrn-row", { button: "right" });
  } else {
    await treeOpen(j, labels);
    await page.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "ContextMenu", code: "ContextMenu", windowsVirtualKeyCode: 93 });
    await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "ContextMenu", code: "ContextMenu", windowsVirtualKeyCode: 93 });
  }
  await page.waitFor(`document.querySelector('.context-menu') !== null`, 5_000);
  for (const english of path) {
    const text = j.t(english);
    const items = `[...document.querySelectorAll('.context-menu [role^=menuitem]')]`;
    await page.waitFor(`${items}.some(b => b.textContent.trim().startsWith(${JSON.stringify(text)}))`, 5_000);
    if (!j.kb) {
      await page.eval(`${items}.find(b => b.textContent.trim().startsWith(${JSON.stringify(text)})).click()`);
      continue;
    }
    const focused = `document.activeElement.textContent.trim().startsWith(${JSON.stringify(text)})`;
    for (let i = 0; i < 60 && !(await page.eval(focused)); i++) {
      await keys.down(page);
      await page.sleep(30);
    }
    assert.equal(await page.eval(focused), true, `the menu entry ${text} was not reached`);
    assert.equal(await page.eval(focusVisible), true, `no visible focus on the menu entry ${text}`);
    if (await page.eval(`document.activeElement.getAttribute('aria-haspopup') !== null`)) await keys.right(page);
    else await keys.enter(page);
  }
}
