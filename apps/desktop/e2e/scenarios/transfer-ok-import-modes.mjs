// Regression guard (passes): imports into existing tables. On PostgreSQL a file with a value
// too long for its column imports nothing (the rows before it are rolled back) and a
// non-number in an integer column names the value; on MySQL the three modes work: add,
// update-or-add (the last row of a repeated key wins, through the staging table) and
// replace.
// Needs PostgreSQL and MySQL from docker/compose.yml.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { chooseFile, closeModal, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const TABLE = "qa_xfer_ok_items";

export const skip = () => skipUnless("pg", "mysql");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "mysql"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, name varchar(10) NOT NULL, price numeric(8,2) DEFAULT 0)`);
  db("mysql", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, name varchar(10) NOT NULL, price decimal(8,2) DEFAULT 0)`);
  writeFileSync(join(dir, "bad.csv"), "ID,Name,Price\n1,pen,1.5\n2,ink,\n3,a-very-long-name,2\n");
  writeFileSync(join(dir, "text.csv"), "ID,Name,Price\nx,pen,1\n");
  writeFileSync(join(dir, "good.csv"), "ID,Name,Price\n1,pen,1.5\n2,ink,\n");
  writeFileSync(join(dir, "upd.csv"), "ID,Name,Price\n2,INK,9.99\n4,new,4\n4,newer,5\n");
}

async function importInto(page, within, file, mode = null) {
  await menu(page, TABLE, "Import data", importDialog, within);
  assert.equal(await chooseFile(page, file), null);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-mapping select').length === 3`, 15_000);
  // Columns are matched by name whatever the case.
  assert.deepEqual(await page.eval(`[...${importDialog}.querySelectorAll('.import-mapping select')].map(s => s.value)`), ["ID", "Name", "Price"]);
  if (mode) await page.eval(`[...${importDialog}.querySelectorAll('.import-modes label')].find(l => l.textContent.startsWith(${JSON.stringify(mode)})).querySelector('input').click()`);
  const out = await runJob(page, importDialog, "Import");
  await closeModal(page, importDialog);
  return out;
}

export default async function (page, { dir }) {
  try {
    const pg = [DBS.pg.name, "public"];
    await showTable(page, DBS.pg.name, "public", TABLE);
    assert.match(await importInto(page, pg, join(dir, "bad.csv")), /Nothing was imported: value too long/);
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}`), "0");
    assert.match(await importInto(page, pg, join(dir, "text.csv")), /Nothing was imported: .*"x"/);

    const my = [DBS.mysql.name, "platypus"];
    await showTable(page, DBS.mysql.name, "platypus", TABLE);
    assert.match(await importInto(page, my, join(dir, "good.csv")), /Imported 2 rows/);
    assert.match(await importInto(page, my, join(dir, "upd.csv"), "Update"), /Imported 3 rows/);
    assert.equal(db("mysql", `SELECT GROUP_CONCAT(CONCAT(id, name, IFNULL(price, '-')) ORDER BY id) FROM ${TABLE}`), "1pen1.50,2INK9.99,4newer5.00");
    assert.match(await importInto(page, my, join(dir, "good.csv"), "Replace"), /Imported 2 rows/);
    assert.equal(db("mysql", `SELECT GROUP_CONCAT(CONCAT(id, name) ORDER BY id) FROM ${TABLE}`), "1pen,2ink");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
    dbQuiet("mysql", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
