// Regression guard (passes): the result export of PostgreSQL values that are easy to get
// wrong. The CSV escapes quotes, commas and newlines and keeps 30-digit decimals, the largest
// bigint and UTC instants; the SQL INSERT statements, run back into a copy of the table, give
// the same rows (text with quotes, backslashes, tabs and emoji, bytea, interval, NaN, dates
// at both ends of the range); the Parquet file imported into a new table keeps the types.
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  chooseFile, closeModal, db, dbQuiet, DBS, exportResult, importDialog, menu, openEditor, runJob, runScript, saveConnections, showTable, skipUnless,
} from "../qa-xfer.mjs";

const TABLE = "qa_xfer_ok_types";
const COLUMNS = "id int PRIMARY KEY, t text, n numeric(30,10), big bigint, d date, tstz timestamptz, b boolean, by bytea, iv interval, f double precision";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db(
    "pg",
    `DROP TABLE IF EXISTS ${TABLE}, ${TABLE}_sql, ${TABLE}_pq; CREATE TABLE ${TABLE} (${COLUMNS});
     INSERT INTO ${TABLE} VALUES
       (1, E'a,"b"\\nline2 ''q'' \\\\ tab\\t😀', 12345678901234567890.0123456789, 9223372036854775807, '2024-02-29', '2024-06-01 10:00:00+02', true, '\\xdeadbeef', '1 day 2 hours', 1.5),
       (2, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL),
       (3, '', -0.0000000001, -9223372036854775808, '0001-01-01', '1999-12-31 23:59:59.999999+00', false, '\\x', '-3 mons', 'NaN');
     CREATE TABLE ${TABLE}_sql (${COLUMNS});`,
  );
}

export default async function (page, { dir }) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await openEditor(page, DBS.pg.name);
    assert.equal(await runScript(page, `SELECT * FROM ${TABLE} ORDER BY id`), "3 rows");

    assert.match(await exportResult(page, join(dir, "out.csv")), /^Exported 3 rows/);
    const csv = readFileSync(join(dir, "out.csv"), "utf8");
    assert.ok(csv.startsWith('id,t,n,big,d,tstz,b,by,iv,f\n1,"a,""b""\nline2'), csv);
    assert.ok(csv.includes(",12345678901234567890.0123456789,9223372036854775807,2024-02-29,2024-06-01T08:00:00Z,true,deadbeef,"), csv);
    assert.ok(csv.includes("\n2,,,,,,,,,\n"), csv);

    assert.match(await exportResult(page, join(dir, "out.sql"), { format: "sql", table: `${TABLE}_sql` }), /^Exported 3 rows/);
    for (const statement of readFileSync(join(dir, "out.sql"), "utf8").split(";\n").filter((s) => s.trim())) db("pg", statement);
    assert.equal(db("pg", `SELECT count(*) FROM (SELECT * FROM ${TABLE} EXCEPT SELECT * FROM ${TABLE}_sql) x`), "0");
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}_sql`), "3");

    assert.match(await exportResult(page, join(dir, `${TABLE}_pq.parquet`), { format: "parquet" }), /^Exported 3 rows/);
    await menu(page, "public", "Import data into a new table", importDialog, DBS.pg.name);
    assert.equal(await chooseFile(page, join(dir, `${TABLE}_pq.parquet`)), null);
    const types = await page.eval(`[...${importDialog}.querySelectorAll('.import-mapping input:not([type])')].filter((_, i) => i % 2).map(i => i.value)`);
    assert.deepEqual(types, ["INTEGER", "TEXT", "DECIMAL(30, 10)", "BIGINT", "DATE", "TIMESTAMPTZ", "BOOLEAN", "BYTEA", "TEXT", "DOUBLE PRECISION"]);
    assert.match(await runJob(page, importDialog, "Import"), /Imported 3 rows/);
    await closeModal(page, importDialog);
    assert.equal(
      db("pg", `SELECT count(*) FROM ${TABLE} a JOIN ${TABLE}_pq b USING (id) WHERE a.t IS NOT DISTINCT FROM b.t AND a.n IS NOT DISTINCT FROM b.n AND a.big IS NOT DISTINCT FROM b.big AND a.tstz IS NOT DISTINCT FROM b.tstz AND a.by IS NOT DISTINCT FROM b.by`),
      "3",
    );
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}, ${TABLE}_sql, ${TABLE}_pq`);
  }
}
