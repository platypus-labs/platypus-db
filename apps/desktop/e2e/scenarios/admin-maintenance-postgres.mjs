// Group S against the dev PostgreSQL: the navigator's Maintenance menu of a table offers Vacuum,
// Analyze and Reindex (no Optimize or Check), each asked first in a dialog (Cancel and Escape
// run nothing), then run in autocommit (VACUUM cannot run in a transaction) and said done with
// the server's VERBOSE output; a table dropped behind the app's back gives the server's error
// in the dialog; a read-only connection refuses and the statistics are not touched.
// See MaintenanceDialog in src/admin/MaintenanceDialog.tsx and the helpers in e2e/admin-maintenance.mjs.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { connections, expand, needs, sh } from "../grid-helpers.mjs";
import { maintenanceFlow, refusedReadOnly } from "../admin-maintenance.mjs";

const DB = "pg";
const NAME = "QA S PostgreSQL maintenance";
const RO_NAME = "QA S PostgreSQL maintenance read-only";
const TABLE = "e2e_maint_t";
const GONE = "e2e_maint_gone";
const RO_TABLE = "e2e_maint_ro";

export const skip = needs(DB);

export function setup({ dir }) {
  sh(
    DB,
    `DROP TABLE IF EXISTS demo.${TABLE}, demo.${GONE}, demo.${RO_TABLE};
     CREATE TABLE demo.${RO_TABLE} (id int PRIMARY KEY);
     INSERT INTO demo.${RO_TABLE} VALUES (1), (2);
     DELETE FROM demo.${RO_TABLE} WHERE id = 2;
     CREATE TABLE demo.${TABLE} (id int PRIMARY KEY, v int);
     CREATE INDEX ${TABLE}_v ON demo.${TABLE} (v);
     INSERT INTO demo.${TABLE} SELECT g, g FROM generate_series(1, 500) g;
     DELETE FROM demo.${TABLE} WHERE id > 250;
     CREATE TABLE demo.${GONE} (id int);`,
  );
  connections(dir, [[NAME, DB], [RO_NAME, DB]]);
  // The second connection is read-only.
  writeReadOnly(dir);
}

function writeReadOnly(dir) {
  const file = join(dir, "connections.toml");
  const parts = readFileSync(file, "utf8").split("[[connections]]");
  parts[2] = `${parts[2].trimEnd()}\nread_only = true\n`;
  writeFileSync(file, parts.join("[[connections]]"));
}

const analyzed = () => sh(DB, `SELECT coalesce(last_analyze::text, 'never') FROM pg_stat_user_tables WHERE relname = '${TABLE}';`);

export default async function (page) {
  try {
    await expand(page, NAME, DB);
    await maintenanceFlow(page, {
      path: [NAME, "platypus", "demo", "Tables", TABLE],
      table: TABLE,
      operations: ["vacuum", "analyze", "reindex"],
      outputs: { vacuum: /vacuuming "platypus\.demo\.e2e_maint_t"/i, analyze: /analyzing "demo\.e2e_maint_t"/i, reindex: null },
      gone: { table: GONE, drop: () => sh(DB, `DROP TABLE demo.${GONE};`) },
      missing: /does not exist/i,
    });
    assert.notEqual(analyzed(), "never", "ANALYZE ran");

    // A read-only connection: refused, and nothing is touched.
    await expand(page, RO_NAME, DB);
    await refusedReadOnly(page, {
      path: [RO_NAME, "platypus", "demo", "Tables", RO_TABLE],
      table: RO_TABLE,
      operation: "vacuum",
    });
    assert.equal(sh(DB, `SELECT coalesce(last_vacuum::text, 'never') FROM pg_stat_user_tables WHERE relname = '${RO_TABLE}';`), "never");
  } finally {
    sh(DB, `DROP TABLE IF EXISTS demo.${TABLE}, demo.${GONE}, demo.${RO_TABLE};`);
  }
}
