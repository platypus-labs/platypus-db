// The value panel (F7) on XML text and on binary values of a SQLite table: XML is indented and
// offered as XML next to Text (broken XML stays text); a PNG shows as an image with a Hex
// view beside it; bytes that are no image open as a hex dump, a long one cut with a count of
// the rest; an empty blob says Empty and NULL says NULL; "Save to file…" writes the exact
// bytes of the cell, and a cancelled dialog writes nothing and shows no error.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { deflateSync } from "node:zlib";
import { CTRL } from "../cdp.mjs";
import { answerSave } from "../transfer-helpers.mjs";
import { openTable, sqliteConnection, visibleTab } from "../query.mjs";
import { crc32 } from "../xlsx.mjs";

const NAME = "Value panel binary";
/** A valid 1 x 1 PNG, made here: signature, IHDR, one deflated pixel, IEND. */
function png() {
  const chunk = (type, data) => {
    const head = Buffer.alloc(4);
    head.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([head, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(1, 0);
  header.writeUInt32BE(1, 4);
  header.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(Buffer.from([0, 255, 128, 0]))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}
const PNG = png();
const SMALL = Buffer.from(Array.from({ length: 32 }, (_, i) => i));
const BIG = Buffer.alloc(70_000, 0xab);
const XML = '<root><item id="1">one</item><empty/></root>';

const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const panel = `document.querySelector('.value-panel')`;
const mode = `(${panel}?.querySelector('select')?.value ?? '')`;
const options = `[...${panel}.querySelectorAll('select option')].map(o => o.value)`;
const shown = `(${panel}?.querySelector('.value-text')?.textContent ?? '')`;
const COLUMNS = ["id", "xml", "img", "small", "big"];

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "binary.db",
    id: "00000000-0000-4000-8000-000000000b71",
    name: NAME,
    sql: "CREATE TABLE blobs (id INTEGER PRIMARY KEY, xml TEXT, img BLOB, small BLOB, big BLOB)",
  });
  const db = new DatabaseSync(join(dir, "binary.db"));
  const insert = db.prepare("INSERT INTO blobs VALUES (?, ?, ?, ?, ?)");
  insert.run(1, XML, PNG, SMALL, BIG);
  insert.run(2, "<a><b></a>", null, Buffer.alloc(0), null);
  db.close();
}

/** Focuses the cell of `column` in row `row` (from 0) and waits for the panel to show it. */
async function focusCell(page, row, column) {
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < COLUMNS.indexOf(column); i++) await page.key("ArrowRight", "ArrowRight", 39);
  if (!(await page.eval(`${panel} !== null`))) await page.key("F7", "F7", 118);
  await page.waitFor(`${panel} !== null && ${panel}.querySelector('.value-name')?.textContent === ${JSON.stringify(column)}`);
  // Loaded: not the "Loading…" state.
  await page.waitFor(`!${panel}.textContent.includes('Loading')`, 10_000);
}

/** Picks `view` in the panel's Show as list. */
async function showAs(page, view) {
  await page.eval(`(() => {
    const select = ${panel}.querySelector('select');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, ${JSON.stringify(view)});
    select.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.waitFor(`${mode} === ${JSON.stringify(view)}`);
}

/** Saves the panel's value to `path` and waits for the file. */
async function saveTo(page, path) {
  await answerSave(page, path);
  await page.eval(`${panel}.querySelector('button[aria-label="Save to file…"]').click()`);
  for (let i = 0; i < 100 && !existsSync(path); i++) await page.sleep(100);
  assert.ok(existsSync(path), `${path} was written`);
}

export default async function (page, { dir }) {
  await openTable(page, NAME, "blobs", "2 rows");

  // XML text: indented, with the XML view next to Text.
  await focusCell(page, 0, "xml");
  await page.waitFor(`${mode} === 'xml'`);
  assert.deepEqual(await page.eval(options), ["text", "xml", "hex"]);
  assert.equal((await page.eval(shown)).replaceAll("\r\n", "\n"), '<root>\n  <item id="1">one</item>\n  <empty/>\n</root>');
  await showAs(page, "text");
  assert.equal(await page.eval(shown), XML);
  // Saved as text, byte for byte.
  await saveTo(page, join(dir, "cell.xml"));
  assert.equal(readFileSync(join(dir, "cell.xml"), "utf8"), XML);

  // XML that does not parse stays text, with no XML view.
  await focusCell(page, 1, "xml");
  await page.waitFor(`${shown} === '<a><b></a>'`);
  assert.equal(await page.eval(mode), "text");
  assert.ok(!(await page.eval(options)).includes("xml"));

  // A PNG: offered as Image (the default) and Hex; the hex view starts with the PNG signature.
  await focusCell(page, 0, "img");
  await page.waitFor(`${mode} === 'image'`);
  assert.deepEqual(await page.eval(options), ["image", "hex"]);
  await showAs(page, "hex");
  assert.match(await page.eval(shown), /^00000000 {2}89 50 4e 47 0d 0a 1a 0a/);
  assert.match(await page.eval(`${panel}.querySelector('.value-size').textContent`), new RegExp(`${PNG.length} bytes`));
  await saveTo(page, join(dir, "pixel.png"));
  assert.deepEqual(readFileSync(join(dir, "pixel.png")), PNG);

  // Bytes that are no image open as a hex dump, with the ASCII column; Image is not offered.
  await focusCell(page, 0, "small");
  await page.waitFor(`${mode} === 'hex'`);
  assert.deepEqual(await page.eval(options), ["hex"]);
  assert.match(await page.eval(shown), /^00000000 {2}00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f {2}\.{16}\n00000010 {2}10 11/);
  await saveTo(page, join(dir, "small.bin"));
  assert.deepEqual(readFileSync(join(dir, "small.bin")), SMALL);

  // A long value: the dump stops at 64 KB and counts the rest; the file has all of it.
  await focusCell(page, 0, "big");
  await page.waitFor(`${shown}.includes('more bytes')`);
  const lines = (await page.eval(shown)).split("\n");
  assert.equal(lines.length, 64 * 1024 / 16 + 1);
  assert.match(lines.at(-1), /… 4,?\d{3} more bytes/);
  await saveTo(page, join(dir, "big.bin"));
  assert.equal(readFileSync(join(dir, "big.bin")).length, BIG.length);

  // An empty blob and a NULL.
  await focusCell(page, 1, "small");
  await page.waitFor(`${panel}.textContent.includes('Empty')`);
  await focusCell(page, 1, "img");
  await page.waitFor(`${panel}.textContent.includes('NULL')`);
  assert.equal(await page.eval(`${panel}.querySelector('img')`), null);

  // A cancelled dialog: nothing written, no error in the panel.
  await answerSave(page, null);
  await page.eval(`${panel}.querySelector('button[aria-label="Save to file…"]').click()`);
  await page.sleep(500);
  assert.equal(await page.eval(`${panel}.querySelector('.error-text') === null`), true);
  assert.equal(existsSync(join(dir, "null")), false);

  // The image itself decodes in the panel (last, so that a failure here leaves the rest checked).
  await focusCell(page, 0, "img");
  await page.waitFor(`${panel}.querySelector('img.value-image') !== null`);
  await page.waitFor(`${panel}.querySelector('img.value-image').naturalWidth === 1`, 10_000);
}

