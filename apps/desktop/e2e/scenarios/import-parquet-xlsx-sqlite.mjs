// Import of a Parquet file and of an Excel workbook into new SQLite tables: a Parquet file
// written by the app's own export is read back with its types and its NULL; a workbook with
// two sheets offers both in a Sheet list, each imported as a table of its own (an empty cell
// is NULL, a column that holds numbers and text stays text, a boolean column is read as
// such); a file that does not exist and a file that is not Parquet show the error and import
// nothing.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, exportResult, expand, fill, importDialog, menu, openEditor, runJob, runScript, sqliteBlock } from "../qa-xfer.mjs";
import { writeXlsx } from "../xlsx.mjs";

const NAME = "Files db";
const sheetSelect = `[...${importDialog}.querySelectorAll('label.field')].find(l => l.textContent.startsWith('Sheet'))?.querySelector('select')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "files.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, price REAL);
    INSERT INTO items VALUES (1, 'pen', 1.5), (2, 'ink', NULL), (3, 'it''s', 10);`);
  db.close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "files.db"))}`);
  writeFileSync(join(dir, "not-parquet.parquet"), "this is not a Parquet file");
  writeXlsx(join(dir, "books.xlsx"), {
    Fruit: [
      ["name", "qty", "fresh"],
      ["apple", 3, true],
      ["pear", null, false],
    ],
    Veg: [
      ["name", "qty"],
      ["kale", 7],
      ["leek", "n/a"],
    ],
  });
}

/** Opens "Import data into a new table…" on the connection's main schema. */
async function openImport(page) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
}

/** Names the new table and imports; returns the outcome text and closes the dialog. */
async function importAs(page, table) {
  await page.eval(fill(`${importDialog}.querySelector('label.field:not(.grow) input')`, table));
  await page.waitFor(`${importDialog}.querySelector('label.field:not(.grow) input').value === ${JSON.stringify(table)}`);
  const out = await runJob(page, importDialog, "Import");
  await closeModal(page, importDialog);
  return out;
}

const rowsOf = (dir, sql) => {
  const db = new DatabaseSync(join(dir, "files.db"));
  const rows = db.prepare(sql).all().map((r) => ({ ...r }));
  db.close();
  return rows;
};

export default async function (page, { dir }) {
  // A Parquet file made by the export of a result.
  await openEditor(page, NAME);
  assert.equal(await runScript(page, "SELECT id, name, price FROM items ORDER BY id"), "3 rows");
  const parquet = join(dir, "items.parquet");
  assert.match(await exportResult(page, parquet, { format: "parquet", source: "loaded" }), /^Exported 3 rows/);

  await openImport(page);
  assert.equal(await chooseFile(page, parquet), null);
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.import-preview tbody tr').length`), 3);
  assert.match(await importAs(page, "from_parquet"), /Imported 3 rows/);
  assert.deepEqual(rowsOf(dir, "SELECT id, name, price FROM from_parquet ORDER BY id"), [
    { id: 1, name: "pen", price: 1.5 },
    { id: 2, name: "ink", price: null },
    { id: 3, name: "it's", price: 10 },
  ]);

  // Files that cannot be read: the error, and nothing imported.
  await openImport(page);
  assert.notEqual(await chooseFile(page, join(dir, "missing.parquet")), null);
  assert.notEqual(await chooseFile(page, join(dir, "not-parquet.parquet")), null);
  assert.equal(await page.eval(`${importDialog}.querySelector('.import-preview') === null`), true);
  await closeModal(page, importDialog);
  assert.equal(rowsOf(dir, "SELECT name FROM sqlite_master WHERE name LIKE 'not%' OR name LIKE 'missing%'").length, 0);

  // A workbook: the Sheet list has both sheets, the first one shows.
  await openImport(page);
  assert.equal(await chooseFile(page, join(dir, "books.xlsx")), null);
  assert.deepEqual(await page.eval(`[...${sheetSelect}.options].map(o => o.textContent)`), ["Fruit", "Veg"]);
  assert.equal(await page.eval(`${sheetSelect}.value`), "Fruit");
  assert.match(await page.eval(`${importDialog}.querySelector('.import-preview').textContent`), /apple/);
  assert.match(await importAs(page, "fruit"), /Imported 2 rows/);
  assert.deepEqual(rowsOf(dir, "SELECT name, qty FROM fruit ORDER BY name"), [
    { name: "apple", qty: 3 },
    { name: "pear", qty: null },
  ]);
  assert.deepEqual(rowsOf(dir, "SELECT name FROM fruit WHERE fresh IN (1, 'true', 'TRUE')").map((r) => r.name), ["apple"]);

  // The second sheet, picked in the list: its rows show, and a column with numbers and text
  // keeps both.
  await openImport(page);
  assert.equal(await chooseFile(page, join(dir, "books.xlsx")), null);
  await page.eval(fill(sheetSelect, "Veg"));
  await page.waitFor(`/kale/.test(${importDialog}.querySelector('.import-preview').textContent)`, 15_000);
  assert.doesNotMatch(await page.eval(`${importDialog}.querySelector('.import-preview').textContent`), /apple/);
  assert.match(await importAs(page, "veg"), /Imported 2 rows/);
  assert.deepEqual(rowsOf(dir, "SELECT name, CAST(qty AS TEXT) AS qty FROM veg ORDER BY name"), [
    { name: "kale", qty: "7" },
    { name: "leek", qty: "n/a" },
  ]);
}
