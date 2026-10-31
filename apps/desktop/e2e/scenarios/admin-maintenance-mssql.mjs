// Group S against the dev SQL Server: the navigator's Maintenance menu of a table offers Analyze
// (UPDATE STATISTICS), Reindex (ALTER INDEX ALL … REBUILD) and Check (DBCC CHECKTABLE), no
// Vacuum or Optimize, each asked first in a dialog (Cancel and Escape run nothing), then run and
// said done with the server's messages (DBCC prints its result as messages); a table dropped
// behind the app's back gives the server's error in the dialog; a read-only connection refuses
// and the indexes are not rebuilt. See MaintenanceDialog in src/admin/MaintenanceDialog.tsx
// and e2e/admin-maintenance.mjs.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { connections, expand, needs, sh } from "../qa-grid.mjs";
import { allParts, maintenanceFlow, refusedReadOnly } from "../admin-maintenance.mjs";

const DB = "mssql";
const NAME = "QA S SQL Server maintenance";
const RO_NAME = "QA S SQL Server maintenance read-only";
const TABLE = "e2e_maint_t";
const GONE = "e2e_maint_gone";
const RO_TABLE = "e2e_maint_ro";

export const skip = needs(DB);

const drop = (...tables) => tables.map((t) => `IF OBJECT_ID('dbo.${t}') IS NOT NULL DROP TABLE dbo.${t};`).join("\n");
const q = (sql) => sh(DB, `SET NOCOUNT ON; ${sql}`);

export function setup({ dir }) {
  sh(
    DB,
    `${drop(TABLE, GONE, RO_TABLE)}
     CREATE TABLE dbo.${RO_TABLE} (id int PRIMARY KEY, v int);
     CREATE INDEX ${RO_TABLE}_v ON dbo.${RO_TABLE} (v);
     INSERT INTO dbo.${RO_TABLE} VALUES (1, 1), (2, 2), (3, 3);
     CREATE TABLE dbo.${TABLE} (id int PRIMARY KEY, v int);
     CREATE INDEX ${TABLE}_v ON dbo.${TABLE} (v);
     INSERT INTO dbo.${TABLE} VALUES (1, 1), (2, 2), (3, 3);
     CREATE TABLE dbo.${GONE} (id int);`,
  );
  connections(dir, [[NAME, DB], [RO_NAME, DB]]);
  const file = join(dir, "connections.toml");
  const parts = readFileSync(file, "utf8").split("[[connections]]");
  parts[2] = `${parts[2].trimEnd()}\nread_only = true\n`;
  writeFileSync(file, parts.join("[[connections]]"));
}

/** When the statistics of the index of the table were last updated. */
const statsDate = (table) => q(`SELECT CONVERT(varchar(30), STATS_DATE(object_id, stats_id), 121) FROM sys.stats WHERE object_id = OBJECT_ID('dbo.${table}') AND name = '${table}_v'`);
/** The index's page count: a rebuild makes a new object, so its id (hobt) changes. */
const hobt = (table) => q(`SELECT CAST(i.index_id AS varchar(10)) + ':' + CAST(p.hobt_id AS varchar(30)) FROM sys.indexes i JOIN sys.partitions p ON p.object_id = i.object_id AND p.index_id = i.index_id WHERE i.object_id = OBJECT_ID('dbo.${table}') AND i.name = '${table}_v'`);

export default async function (page) {
  try {
    await expand(page, NAME, DB);
    await allParts(
      () =>
        maintenanceFlow(page, {
          path: [NAME, "platypus", "dbo", "Tables", TABLE],
          table: TABLE,
          operations: ["analyze", "reindex", "check"],
          outputs: { analyze: null, reindex: null, check: /DBCC results for .e2e_maint_t.[sS]*There are 3 rows in 1 pages/ },
          gone: { table: GONE, drop: () => sh(DB, drop(GONE)) },
          missing: /Cannot find the object|does not exist|e2e_maint_gone/i,
        }),
      async () => {
        // A read-only connection: refused, and the index is not rebuilt.
        const before = hobt(RO_TABLE);
        assert.ok(before.includes(":"), before);
        const stats = statsDate(RO_TABLE);
        await expand(page, RO_NAME, DB);
        await refusedReadOnly(page, { path: [RO_NAME, "platypus", "dbo", "Tables", RO_TABLE], table: RO_TABLE, operation: "reindex" });
        assert.equal(hobt(RO_TABLE), before, "REBUILD ran on a read-only connection");
        assert.equal(statsDate(RO_TABLE), stats);
      },
    );
  } finally {
    sh(DB, drop(TABLE, GONE, RO_TABLE));
  }
}
