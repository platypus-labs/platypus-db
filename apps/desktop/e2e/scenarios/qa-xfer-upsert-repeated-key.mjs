// XFER-10 (medium). An upsert import fails on PostgreSQL when the file repeats a key.
// Steps: a PostgreSQL table (id int PRIMARY KEY, name text) holding (1,'a'), (2,'b'); a CSV
// `id,name / 2,B / 3,c / 3,C`; navigator › table › "Import data…", "Update rows with the
// same key, add the others", Import.
// Expected: as on MySQL, where the import keeps the last row of each key: the table holds
// (1,a) (2,B) (3,C).
// Actual: "Nothing was imported: ON CONFLICT DO UPDATE command cannot affect row a second
// time". SQL Server fails too ("Violation of PRIMARY KEY constraint … duplicate key value is
// (3)"). Exports and logs often repeat a key, the later row being the current one.
// Suspected: crates/platypus-transfer/src/import.rs:259-266 (last_per_key() only runs for the
// MySQL staging path).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { chooseFile, closeModal, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_upsert";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, name text); INSERT INTO ${TABLE} VALUES (1, 'a'), (2, 'b')`);
  writeFileSync(join(dir, "changes.csv"), "id,name\n2,B\n3,c\n3,C\n");
}

export default async function (page, { dir }) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await menu(page, TABLE, "Import data", importDialog, [DBS.pg.name, "public"]);
    assert.equal(await chooseFile(page, join(dir, "changes.csv")), null);
    await page.waitFor(`${importDialog}.querySelectorAll('.import-mapping select').length === 2`, 15_000);
    await page.eval(`[...${importDialog}.querySelectorAll('.import-modes label')].find(l => l.textContent.startsWith('Update')).querySelector('input').click()`);
    const out = await runJob(page, importDialog, "Import");
    await closeModal(page, importDialog);
    assert.match(out, /^Imported/);
    assert.equal(db("pg", `SELECT string_agg(id || name, ' ' ORDER BY id) FROM ${TABLE}`), "1a 2B 3C");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
