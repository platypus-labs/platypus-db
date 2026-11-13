// Transfer profiles of imports and copies (group H) between SQLite files: a CSV import into
// an existing table (mode and mapping kept, the file read again at each run), opened again
// with its choices and with a notice for a column dropped since; a table copied to another
// database; and a whole-schema migration saved as "all tables", so a table added to the
// schema after the save is copied by the run too.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { openProfiles, rowAction, runProfile, saveProfile, savedProfiles } from "../profiles-helpers.mjs";
import { button, chooseFile, closeModal, copyTarget, importDialog, menu, migrateDialog, saveConnections, showTable, sqliteBlock } from "../transfer-helpers.mjs";

const SRC = "Profiles source";
const MIG = "Profiles migrated";
const DST = "Profiles target";

export function setup({ dir }) {
  const src = new DatabaseSync(join(dir, "src.db"));
  src.exec(`
    CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, price REAL);
    CREATE TABLE t1 (id INTEGER PRIMARY KEY, v TEXT);
    CREATE TABLE t2 (id INTEGER PRIMARY KEY, v TEXT);
    INSERT INTO items (name, price) VALUES ('pen', 1.5), ('ink', 0.25), ('cap', 10);
    INSERT INTO t1 (v) VALUES ('a'), ('b');
    INSERT INTO t2 (v) VALUES ('c');
  `);
  src.close();
  new DatabaseSync(join(dir, "mig.db")).close();
  const dst = new DatabaseSync(join(dir, "dst.db"));
  dst.exec("CREATE TABLE stock (id INTEGER PRIMARY KEY, name TEXT, price REAL)");
  dst.close();
  writeFileSync(join(dir, "stock.csv"), "ID,Name,price,extra\n1,pen,1.5,x\n2,ink,0.25,y\n3,cap,10,z\n");
  // The order is the order of the targets a dialog offers first.
  saveConnections(
    dir,
    [],
    sqliteBlock(SRC, join(dir, "src.db"), "00000000-0000-4000-8000-0000000000b1") +
      sqliteBlock(MIG, join(dir, "mig.db"), "00000000-0000-4000-8000-0000000000b2") +
      sqliteBlock(DST, join(dir, "dst.db"), "00000000-0000-4000-8000-0000000000b3"),
  );
}

/** The rows of `sql` in the database file `name` of `dir`. */
function rows(dir, name, sql) {
  const db = new DatabaseSync(join(dir, name));
  try {
    return db.prepare(sql).all().map((r) => ({ ...r }));
  } finally {
    db.close();
  }
}

const modeInput = (mode) =>
  `[...${importDialog}.querySelectorAll('.import-modes label')].find(l => l.textContent.startsWith(${JSON.stringify(mode)})).querySelector('input')`;

export default async function (page, { dir }) {
  const csv = join(dir, "stock.csv");

  // A CSV into the table stock, replacing its rows; saved from the import dialog.
  await showTable(page, DST, "main", "stock");
  await menu(page, "stock", "Import data", importDialog, DST);
  assert.equal(await chooseFile(page, csv), null);
  await page.waitFor(`${importDialog}.querySelector('.import-modes') !== null`, 15_000);
  await page.eval(`${modeInput("Replace")}.click()`);
  await page.waitFor(`${modeInput("Replace")}.checked`);
  assert.equal(await saveProfile(page, importDialog, "Save as profile…", "Stock from CSV"), null);
  await closeModal(page, importDialog);
  const kept = savedProfiles(dir)["Stock from CSV"];
  assert.equal(kept.task.kind, "import_file");
  assert.equal(kept.task.path, csv);
  assert.equal(kept.task.plan.mode, "replace");
  assert.equal(kept.task.plan.table, "stock");

  // Run from the list: the three rows. The file is read at each run: with two rows in it,
  // the table is replaced by two.
  let run = await runProfile(page, "Stock from CSV");
  assert.equal(run.error, null);
  assert.match(run.text, /Done: 3 rows\./);
  assert.deepEqual(rows(dir, "dst.db", "SELECT id, name, price FROM stock ORDER BY id"), [
    { id: 1, name: "pen", price: 1.5 },
    { id: 2, name: "ink", price: 0.25 },
    { id: 3, name: "cap", price: 10 },
  ]);
  writeFileSync(csv, "ID,Name,price,extra\n7,mug,4,q\n8,jar,2,r\n");
  run = await runProfile(page, "Stock from CSV");
  assert.equal(run.error, null);
  assert.match(run.text, /Done: 2 rows\./);
  assert.deepEqual(rows(dir, "dst.db", "SELECT id, name FROM stock ORDER BY id"), [
    { id: 7, name: "mug" },
    { id: 8, name: "jar" },
  ]);

  // Open: the file, the mode and the mapping come back (columns match whatever the case).
  await openProfiles(page);
  await rowAction(page, "Stock from CSV", "Open…");
  await page.waitFor(`${importDialog} !== null && ${importDialog}.querySelectorAll('.import-preview tbody tr').length === 2`, 20_000);
  assert.equal(await page.eval(`${importDialog}.querySelector('input').value`), csv);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-mapping select').length === 3`, 15_000);
  assert.deepEqual(await page.eval(`[...${importDialog}.querySelectorAll('.import-mapping select')].map(s => s.value)`), ["ID", "Name", "price"]);
  assert.equal(await page.eval(`${modeInput("Replace")}.checked`), true);
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.notice.warn').length`), 0);
  await closeModal(page, importDialog);

  // A column of the table dropped since: the dialog says so, and the others stay mapped.
  const dst = new DatabaseSync(join(dir, "dst.db"));
  dst.exec("ALTER TABLE stock DROP COLUMN price");
  dst.close();
  await openProfiles(page);
  await rowAction(page, "Stock from CSV", "Open…");
  await page.waitFor(`${importDialog} !== null && ${importDialog}.textContent.includes('No longer found, left out: price.')`, 20_000);
  assert.deepEqual(await page.eval(`[...${importDialog}.querySelectorAll('.import-mapping select')].map(s => s.value)`), ["ID", "Name"]);
  await closeModal(page, importDialog);

  // A table copied to another database, new there; the profile copies it again from the list.
  await showTable(page, SRC, "main", "items");
  await menu(page, "items", "Copy to a database", importDialog, SRC);
  assert.equal(await copyTarget(page, DST), null);
  assert.equal(await saveProfile(page, importDialog, "Save as profile…", "Items to target"), null);
  await closeModal(page, importDialog);
  assert.equal(savedProfiles(dir)["Items to target"].task.kind, "transfer_table");
  run = await runProfile(page, "Items to target");
  assert.equal(run.error, null);
  assert.match(run.text, /Done: 3 rows\./);
  assert.deepEqual(rows(dir, "dst.db", "SELECT id, name, price FROM items ORDER BY id"), [
    { id: 1, name: "pen", price: 1.5 },
    { id: 2, name: "ink", price: 0.25 },
    { id: 3, name: "cap", price: 10 },
  ]);

  // The same table copied again, now into the table that is there: the plan keeps it as an
  // existing table, not as one to create.
  await showTable(page, SRC, "main", "items");
  await menu(page, "items", "Copy to a database", importDialog, SRC);
  assert.equal(await copyTarget(page, DST), null);
  assert.equal(await saveProfile(page, importDialog, "Save as profile…", "Items into existing"), null);
  await closeModal(page, importDialog);
  const into = savedProfiles(dir)["Items into existing"].task;
  assert.equal(into.plan.create, false, "a copy into an existing table does not create it");
  assert.equal(into.plan.table, "items");

  // The whole schema, saved as "all tables".
  await menu(page, "main", "Copy schema to a database", migrateDialog, SRC);
  await page.waitFor(`${migrateDialog}.querySelector('select')?.selectedOptions[0]?.textContent === ${JSON.stringify(MIG)}`, 15_000);
  await page.waitFor(`['items', 't1', 't2'].every(n => ${migrateDialog}.textContent.includes(n)) && !${button(migrateDialog, "Copy")}.disabled`, 20_000);
  assert.equal(await saveProfile(page, migrateDialog, "Save as profile…", "Schema to migrated"), null);
  await closeModal(page, migrateDialog);
  const whole = savedProfiles(dir)["Schema to migrated"].task;
  assert.equal(whole.kind, "migrate");
  assert.equal(whole.tables, null, "a whole schema keeps no list of tables");
  assert.equal(whole.objects, true);

  // A table added to the schema after the save is copied too; the dialog lists it as well.
  const src = new DatabaseSync(join(dir, "src.db"));
  src.exec("CREATE TABLE t3 (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t3 (v) VALUES ('x'), ('y'), ('z'), ('w')");
  src.close();
  await openProfiles(page);
  await rowAction(page, "Schema to migrated", "Open…");
  await page.waitFor(`${migrateDialog} !== null && ${migrateDialog}.textContent.includes('t3')`, 20_000);
  await closeModal(page, migrateDialog);
  run = await runProfile(page, "Schema to migrated");
  assert.equal(run.error, null, run.text);
  assert.match(run.text, /Done: [0-9]+ rows\./);
  assert.deepEqual(
    rows(dir, "mig.db", "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map((r) => r.name),
    ["items", "t1", "t2", "t3"],
  );
  assert.equal(rows(dir, "mig.db", "SELECT count(*) AS n FROM t3")[0].n, 4);
  assert.equal(rows(dir, "mig.db", "SELECT count(*) AS n FROM items")[0].n, 3);
}
