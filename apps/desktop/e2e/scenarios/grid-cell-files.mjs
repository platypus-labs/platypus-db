// Files held by cells of a SQLite table: a binary cell that is a PDF, a PNG or a DOCX-like ZIP
// shows its type and size in the grid in place of hex, in any column; the grid menu offers
// "Save PDF…" for one such cell (the bytes written are the cell's) and "Save N files to a
// folder…" for a selection down a column (named from the row's key column, never overwriting,
// `-2` for a taken name); text cells that hold a `data:` URL, base64 (URL-safe, with line
// breaks) or hex (`\x…`, plain) are files too and are saved decoded; random bytes, plain text,
// a hash and a UUID, a NULL are not offered; the value panel has a "Save PDF…" button; a
// masked column offers nothing until masking is off; a connection that keeps its data in
// offers nothing and its panel button is disabled.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL, SHIFT } from "../cdp.mjs";
import { answerSave } from "../transfer-helpers.mjs";
import { openTable, treeRow, visibleTab } from "../query.mjs";

const NAME = "Cell files";
const KEPT = "Cell files kept";
const COLUMNS = ["doc_id", "blob", "as_url", "as_b64", "as_hex", "note", "secret_scan"];

/** `n` bytes after `head`, a pattern that is no magic number. */
const filled = (head, n) => Buffer.concat([Buffer.from(head, "latin1"), Buffer.from(Array.from({ length: n - head.length }, (_, i) => (i * 7 + 3) % 251))]);
const PDF_A = filled("%PDF-1.4\n", 2400);
const PDF_B = filled("%PDF-1.7\n", 300);
const PNG = filled("\x89PNG\r\n\x1a\n", 900);
const RANDOM = Buffer.from(Array.from({ length: 64 }, (_, i) => (i * 37 + 11) % 256));

function zipEntry(name, data) {
  const head = Buffer.alloc(30);
  head.write("PK\x03\x04", 0, "latin1");
  head.writeUInt32LE(data.length, 18);
  head.writeUInt32LE(data.length, 22);
  head.writeUInt16LE(name.length, 26);
  return Buffer.concat([head, Buffer.from(name), data]);
}
const DOCX = Buffer.concat([zipEntry("[Content_Types].xml", Buffer.from("<Types/>")), zipEntry("word/document.xml", Buffer.from("<w:document/>".repeat(40)))]);

const human = (n) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`);
const MD5 = "d41d8cd98f00b204e9800998ecf8427e";
const UUID = "550e8400-e29b-41d4-a716-446655440000";
const b64 = (buf) => buf.toString("base64");
const rows = [
  // doc_id, blob, as_url, as_b64, as_hex, note, secret_scan
  [7, PDF_A, `data:image/png;base64,${b64(PNG)}`, b64(PDF_A), `\\x${PNG.toString("hex")}`, "hello world", PDF_A],
  [7, PDF_B, null, b64(DOCX).replaceAll("+", "-").replaceAll("/", "_").replace(/(.{60})/g, "$1\r\n"), `0x${PDF_B.toString("hex")}`, MD5, PDF_B],
  [8, DOCX, `data:application/pdf;base64,${b64(PDF_B)}`, null, `\\x${PDF_A.toString("hex")}`, UUID, null],
  [9, PNG, null, b64(Buffer.from("just some ordinary words written in a text column")), PNG.toString("hex"), "plain text", null],
  [10, RANDOM, null, null, "deadbeefdeadbeef", "x", null],
  [11, null, null, null, null, null, null],
];

export function setup({ dir }) {
  for (const [file, table] of [["files.db", "docs"], ["kept.db", "kept_docs"]]) {
    const db = new DatabaseSync(join(dir, file));
    db.exec(`CREATE TABLE ${table} (${COLUMNS[0]} INTEGER, blob BLOB, as_url TEXT, as_b64 VARCHAR, as_hex TEXT, note TEXT, secret_scan BLOB)`);
    const insert = db.prepare(`INSERT INTO ${table} VALUES (?, ?, ?, ?, ?, ?, ?)`);
    // The kept connection has one row less, so that its tab is told from the first by its count.
    for (const r of table === "docs" ? rows : rows.slice(0, 5)) insert.run(...r);
    db.close();
  }
  writeFileSync(join(dir, "settings.toml"), "[safety]\nmask = true\n");
  const c = (id, name, file, extra) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000cf0${id}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false
${extra}`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${c(1, NAME, "files.db", "")}${c(2, KEPT, "kept.db", "\n[connections.restrictions]\nno_export = true\n")}`);
}

const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const panel = `${visibleTab}.querySelector('.value-panel')`;
const items = `[...document.querySelectorAll('.context-menu button')].map(b => b.querySelector('span')?.textContent)`;
const toasts = `[...document.querySelectorAll('.toaster .toast')].map(t => t.textContent)`;

/** Focuses the cell of `column` in row `row` (from 0). */
async function focusCell(page, row, column, rowsDown = 0) {
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < COLUMNS.indexOf(column); i++) await page.key("ArrowRight", "ArrowRight", 39);
  for (let i = 0; i < rowsDown; i++) await page.key("ArrowDown", "ArrowDown", 40, SHIFT);
}

/** Opens the grid's menu on the row numbers, which keep the selection; the items it shows. */
async function menuItems(page) {
  await page.eval(`document.querySelectorAll('#e2e-grid').forEach(e => e.removeAttribute('id')); ${grid}.id = 'e2e-grid'`);
  const r = await page.rect("#e2e-grid");
  await page.click(r.x + 4, r.y + 52, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(items);
}

async function closeMenu(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

/** The menu of the cell at `row`/`column`; closed again. */
async function menuOf(page, row, column, rowsDown = 0) {
  await focusCell(page, row, column, rowsDown);
  const found = await menuItems(page);
  await closeMenu(page);
  return found;
}

async function waitForFile(page, path) {
  for (let i = 0; i < 100 && !existsSync(path); i++) await page.sleep(100);
  assert.ok(existsSync(path), `${path} was written`);
}

/** Chooses the menu item `label` with the dialog answering `path`; the file written. */
async function saveFromMenu(page, row, column, label, path) {
  await focusCell(page, row, column);
  const found = await menuItems(page);
  assert.ok(found.includes(label), `${label} in ${found}`);
  await answerSave(page, path);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)}).click()`);
  await waitForFile(page, path);
  return readFileSync(path);
}

const saveItem = (items, prefix) => items.filter((i) => i?.startsWith(prefix) && i !== "Save result…");

export default async function (page, { dir }) {
  // What the grid draws, to read its labels: the canvas has no text to query.
  await page.eval(`(() => {
    window.__drawn = new Set();
    const original = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function (text, ...rest) { window.__drawn.add(String(text)); return original.call(this, text, ...rest); };
  })()`);
  await openTable(page, NAME, "docs", "6 rows");
  await page.waitFor(`window.__drawn.has('PDF · ${human(PDF_A.length)}')`, 15_000);

  // Binary cells that are files show their type and size; the rest keep their hex.
  const drawn = await page.eval(`[...window.__drawn]`);
  for (const label of [`PDF · ${human(PDF_A.length)}`, `PDF · ${human(PDF_B.length)}`, `DOCX · ${human(DOCX.length)}`, `PNG · ${human(PNG.length)}`]) {
    assert.ok(drawn.includes(label), `${label} in ${drawn.filter((d) => d.includes("·"))}`);
  }
  assert.ok(drawn.some((d) => d.startsWith(RANDOM.toString("hex").slice(0, 16))), "random bytes stay hex");

  // Masking is on and the screen may be shared: the sensitive column offers nothing.
  const masked = await menuOf(page, 0, "secret_scan");
  assert.deepEqual(saveItem(masked, "Save "), [], `${masked}`);
  // ...while the others do. One cell: the type is in the label, in any type of column.
  assert.ok((await menuOf(page, 0, "blob")).includes("Save PDF…"));
  assert.ok((await menuOf(page, 1, "blob")).includes("Save PDF…"));
  assert.ok((await menuOf(page, 2, "blob")).includes("Save DOCX…"));
  assert.ok((await menuOf(page, 3, "blob")).includes("Save PNG…"));
  assert.ok((await menuOf(page, 0, "as_url")).includes("Save PNG…"), "a data URL in a TEXT column");
  assert.ok((await menuOf(page, 2, "as_url")).includes("Save PDF…"), "a data URL declaring a PDF");
  assert.ok((await menuOf(page, 1, "as_b64")).includes("Save DOCX…"), "URL-safe base64 with line breaks in a VARCHAR");
  assert.ok((await menuOf(page, 0, "as_hex")).includes("Save PNG…"), "bytea text, \\x and hex");
  assert.ok((await menuOf(page, 1, "as_hex")).includes("Save PDF…"), "0x and hex");
  assert.ok((await menuOf(page, 3, "as_hex")).includes("Save PNG…"), "plain hex");

  // Not files: random bytes, plain text, a hash, a UUID, words in base64, a short hex string, NULL.
  const none = [[4, "blob"], [5, "blob"], [0, "note"], [1, "note"], [2, "note"], [3, "note"], [3, "as_b64"], [4, "as_hex"], [5, "as_hex"], [1, "as_url"], [0, "doc_id"]];
  for (const [row, column] of none) {
    const found = await menuOf(page, row, column);
    assert.deepEqual(saveItem(found, "Save "), [], `row ${row + 1} ${column}: ${found}`);
  }
  // The menu keeps its other items, with none shifting: Copy is still the first.
  assert.equal((await menuOf(page, 4, "blob"))[0], "Copy");

  // Saving one cell writes its bytes: as stored, or decoded from the text.
  const dest = (name) => join(dir, name);
  assert.deepEqual(await saveFromMenu(page, 0, "blob", "Save PDF…", dest("one.pdf")), PDF_A);
  assert.deepEqual(await saveFromMenu(page, 3, "blob", "Save PNG…", dest("two.png")), PNG);
  assert.deepEqual(await saveFromMenu(page, 0, "as_url", "Save PNG…", dest("url.png")), PNG);
  assert.deepEqual(await saveFromMenu(page, 1, "as_b64", "Save DOCX…", dest("b64.docx")), DOCX);
  assert.deepEqual(await saveFromMenu(page, 0, "as_hex", "Save PNG…", dest("pg.png")), PNG);
  assert.deepEqual(await saveFromMenu(page, 1, "as_hex", "Save PDF…", dest("hex.pdf")), PDF_B);
  await page.waitFor(`${toasts}.some(t => t.startsWith('Saved '))`);
  // A cancelled dialog writes nothing.
  await focusCell(page, 0, "blob");
  await menuItems(page);
  await answerSave(page, null);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === 'Save PDF…').click()`);
  await page.sleep(400);
  assert.equal(existsSync(dest("null")), false);

  // Several cells down a column: the files go to a folder, named from doc_id, never over a file.
  const folder = dest("many");
  mkdirSync(folder);
  writeFileSync(join(folder, "8.docx"), "mine");
  await focusCell(page, 0, "blob", 5);
  const several = await menuItems(page);
  assert.ok(several.includes("Save 4 files to a folder…"), `${several}`);
  assert.deepEqual(saveItem(several, "Save PDF"), [], "the single item is for one cell");
  await page.eval(`window.__platypus.answer('folder', ${JSON.stringify(folder)})`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === 'Save 4 files to a folder…').click()`);
  await page.waitFor(`${toasts}.some(t => t.startsWith('Saved 4 files to '))`, 20_000);
  assert.deepEqual(readdirSync(folder).sort(), ["7-2.pdf", "7.pdf", "8-2.docx", "8.docx", "9.png"]);
  assert.equal(readFileSync(join(folder, "8.docx"), "utf8"), "mine", "an existing file is never replaced");
  assert.deepEqual(readFileSync(join(folder, "7.pdf")), PDF_A);
  assert.deepEqual(readFileSync(join(folder, "7-2.pdf")), PDF_B);
  assert.deepEqual(readFileSync(join(folder, "8-2.docx")), DOCX);
  assert.deepEqual(readFileSync(join(folder, "9.png")), PNG);

  // The same into the same folder again: every name is taken, so each gets the next suffix.
  await focusCell(page, 0, "blob", 5);
  await menuItems(page);
  await page.eval(`window.__platypus.answer('folder', ${JSON.stringify(folder)})`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === 'Save 4 files to a folder…').click()`);
  await waitForFile(page, join(folder, "9-2.png"));
  assert.deepEqual(readdirSync(folder).sort(), ["7-2.pdf", "7-3.pdf", "7-4.pdf", "7.pdf", "8-2.docx", "8-3.docx", "8.docx", "9-2.png", "9.png"]);

  // Text cells: two hex files in a folder; a folder that is a file is an error and writes nothing.
  const hexFolder = dest("hex");
  mkdirSync(hexFolder);
  await focusCell(page, 2, "as_hex", 1);
  assert.ok((await menuItems(page)).includes("Save 2 files to a folder…"));
  await page.eval(`window.__platypus.answer('folder', ${JSON.stringify(hexFolder)})`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === 'Save 2 files to a folder…').click()`);
  await waitForFile(page, join(hexFolder, "9.png"));
  assert.deepEqual(readdirSync(hexFolder).sort(), ["8.pdf", "9.png"]);
  assert.deepEqual(readFileSync(join(hexFolder, "8.pdf")), PDF_A);

  await focusCell(page, 2, "as_hex", 1);
  await menuItems(page);
  await page.eval(`window.__platypus.answer('folder', ${JSON.stringify(dest("files.db"))})`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === 'Save 2 files to a folder…').click()`);
  await page.waitFor(`${toasts}.some(t => t.includes('not a folder'))`, 15_000);

  // A selection holding a single file offers neither item; one with no file, none.
  const one = await menuOf(page, 3, "blob", 2);
  assert.deepEqual(saveItem(one, "Save "), [], `${one}`);
  assert.deepEqual(saveItem(await menuOf(page, 4, "blob", 1), "Save "), []);
  // Several columns: nothing.
  await focusCell(page, 0, "blob");
  await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
  assert.deepEqual(saveItem(await menuItems(page), "Save "), []);
  await closeMenu(page);

  // The value panel: a Save button named for the type, writing the same bytes; text cells too.
  await focusCell(page, 0, "blob");
  await page.key("F7", "F7", 118);
  const button = `[...${panel}.querySelectorAll('.value-file button')]`;
  await page.waitFor(`${panel} !== null && ${button}.some(b => b.textContent.includes('Save PDF…'))`, 10_000);
  await answerSave(page, dest("panel.pdf"));
  await page.eval(`${button}.find(b => b.textContent.includes('Save PDF…')).click()`);
  await waitForFile(page, dest("panel.pdf"));
  assert.deepEqual(readFileSync(dest("panel.pdf")), PDF_A);
  await focusCell(page, 0, "as_url");
  await page.waitFor(`${button}.some(b => b.textContent.includes('Save PNG…'))`, 10_000);
  await answerSave(page, dest("panel-url.png"));
  await page.eval(`${button}.find(b => b.textContent.includes('Save PNG…')).click()`);
  await waitForFile(page, dest("panel-url.png"));
  assert.deepEqual(readFileSync(dest("panel-url.png")), PNG);
  // Not a file, or NULL: no button.
  for (const [row, column] of [[4, "blob"], [0, "note"], [5, "blob"]]) {
    await focusCell(page, row, column);
    await page.waitFor(`${panel}.querySelector('.value-name')?.textContent === ${JSON.stringify(column)} && !${panel}.textContent.includes('Loading')`, 10_000);
    await page.sleep(300);
    assert.equal(await page.eval(`${button}.length`), 0, `${column} row ${row + 1}`);
  }
  // A masked column: no button either.
  await focusCell(page, 0, "secret_scan");
  await page.waitFor(`${panel}.textContent.includes('Masked while the screen may be shared')`, 10_000);
  await page.sleep(300);
  assert.equal(await page.eval(`${button}.length`), 0);

  // Masking off: the column offers its file.
  await page.key("m", "KeyM", 77, CTRL | ALT);
  await page.waitFor(`document.querySelector('.topbar-masked') === null`, 15_000);
  assert.ok((await menuOf(page, 0, "secret_scan")).includes("Save PDF…"));
  await page.waitFor(`${button}.some(b => b.textContent.includes('Save PDF…'))`, 10_000);
  // Closing the panel keeps the grid for the next connection.
  await page.key("F7", "F7", 118);

  // A connection that keeps its data in: no items, and the panel's button is disabled.
  await page.eval(`document.getElementById('e2e-table')?.removeAttribute('id')`);
  await openTable(page, KEPT, "kept_docs", "5 rows");
  await page.eval(`document.querySelectorAll('#e2e-grid').forEach(e => e.removeAttribute('id'))`);
  const kept = await menuOf(page, 0, "blob");
  assert.deepEqual(saveItem(kept, "Save "), [], `${kept}`);
  const keptMany = await (async () => {
    await focusCell(page, 0, "blob", 5);
    const found = await menuItems(page);
    await closeMenu(page);
    return found;
  })();
  assert.deepEqual(saveItem(keptMany, "Save "), [], `${keptMany}`);
  await focusCell(page, 0, "blob");
  await page.key("F7", "F7", 118);
  await page.waitFor(`${panel} !== null && ${button}.some(b => b.textContent.includes('Save PDF…') && b.disabled)`, 10_000);
  assert.equal(await page.eval(`${button}.find(b => b.textContent.includes('Save PDF…')).title`), "This connection keeps its data in: nothing is saved to files.");
  assert.equal(await page.eval(`${treeRow(NAME)} !== undefined`), true);
}
