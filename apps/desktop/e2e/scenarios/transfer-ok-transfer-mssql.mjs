// Regression guard (passes): a PostgreSQL table copied into a new SQL Server table keeps its
// values — unicode text with emoji, quotes and newlines (NVARCHAR), numeric(12,4), the bigint
// beyond 2^53, timestamp(3), the zoned instant (DATETIMEOFFSET), boolean (BIT), bytea
// (VARBINARY) and NULLs.
// Needs PostgreSQL and SQL Server from docker/compose.yml.
import assert from "node:assert/strict";
import { closeModal, copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const TABLE = "qa_xfer_ok_ms";

export const skip = () => skipUnless("pg", "mssql");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "mssql"]);
  db(
    "pg",
    `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, note text, price numeric(12,4), big bigint, ts timestamp(3), tstz timestamptz, ok boolean, raw bytea);
     INSERT INTO ${TABLE} VALUES (1, E'it''s\\n日本😀', 1234.5678, 9007199254740993, '2024-01-01 12:34:56.789', '2024-06-01 10:00:00+02', true, '\\xdeadbeef'),
                                 (2, NULL, NULL, NULL, NULL, NULL, NULL, NULL);`,
  );
  db("mssql", `DROP TABLE IF EXISTS ${TABLE}`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await menu(page, TABLE, "Copy to a database", importDialog, [DBS.pg.name, "public"]);
    assert.equal(await copyTarget(page, DBS.mssql.name), null);
    const types = await page.eval(`[...${importDialog}.querySelectorAll('.import-mapping input:not([type])')].filter((_, i) => i % 2).map(i => i.value)`);
    assert.deepEqual(types, ["INT", "NVARCHAR(MAX)", "DECIMAL(12, 4)", "BIGINT", "DATETIME2", "DATETIMEOFFSET", "BIT", "VARBINARY(MAX)"]);
    assert.match(await runJob(page, importDialog, "Copy"), /Imported 2 rows/);
    await closeModal(page, importDialog);
    const got = db(
      "mssql",
      `SELECT CONVERT(varchar(max), CAST(note AS varbinary(max)), 2), price, big, CONVERT(varchar(30), ts, 126), CONVERT(varchar(40), tstz, 127), ok, CONVERT(varchar(20), raw, 2) FROM ${TABLE} WHERE id = 1`,
    );
    // UTF-16LE of "it's\n日本😀".
    assert.equal(got, "69007400270073000A00E5652C673DD800DE\t1234.5678\t9007199254740993\t2024-01-01T12:34:56.7890000\t2024-06-01T08:00:00Z\t1\tDEADBEEF");
    assert.equal(db("mssql", `SELECT count(*) FROM ${TABLE} WHERE id = 2 AND note IS NULL AND raw IS NULL AND tstz IS NULL`), "1");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
    dbQuiet("mssql", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
