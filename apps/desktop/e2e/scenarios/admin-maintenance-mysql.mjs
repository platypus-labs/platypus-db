// Group S against the dev MySQL: the navigator's Maintenance menu of a table offers Analyze,
// Optimize and Check (no Vacuum or Reindex), each asked first in a dialog (Cancel and Escape run
// nothing), then run and said done with the server's status rows (`status · OK`); a table that
// does not exist gives an error in the dialog (MySQL answers ANALYZE TABLE of a missing table
// with rows saying so, not with an error); a read-only connection refuses and the table is not
// touched. See MaintenanceDialog in src/admin/MaintenanceDialog.tsx and e2e/admin-maintenance.mjs.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { connections, expand, needs, sh } from "../qa-grid.mjs";
import {
  allParts,
  maintenanceFlow,
  refusedReadOnly,
} from "../admin-maintenance.mjs";

const DB = "mysql";
const NAME = "QA S MySQL maintenance";
const RO_NAME = "QA S MySQL maintenance read-only";
const TABLE = "e2e_maint_t";
const GONE = "e2e_maint_gone";
const RO_TABLE = "e2e_maint_ro";

export const skip = needs(DB);

export function setup({ dir }) {
  sh(
    DB,
    `DROP TABLE IF EXISTS ${TABLE}, ${GONE}, ${RO_TABLE};
     CREATE TABLE ${RO_TABLE} (id int PRIMARY KEY, v int);
     INSERT INTO ${RO_TABLE} VALUES (1, 1), (2, 2), (3, 3);
     CREATE TABLE ${TABLE} (id int PRIMARY KEY, v int);
     CREATE INDEX ${TABLE}_v ON ${TABLE} (v);
     INSERT INTO ${TABLE} VALUES (1, 1), (2, 2), (3, 3);
     DELETE FROM ${TABLE} WHERE id = 3;
     CREATE TABLE ${GONE} (id int);`,
  );
  connections(dir, [
    [NAME, DB],
    [RO_NAME, DB],
  ]);
  const file = join(dir, "connections.toml");
  const parts = readFileSync(file, "utf8").split("[[connections]]");
  parts[2] = `${parts[2].trimEnd()}\nread_only = true\n`;
  writeFileSync(file, parts.join("[[connections]]"));
}

export default async function (page) {
  try {
    await expand(page, NAME, DB);
    await allParts(
      () =>
        maintenanceFlow(page, {
          path: [NAME, "platypus", "Tables", TABLE],
          table: TABLE,
          operations: ["analyze", "optimize", "check"],
          outputs: {
            analyze: /status · OK/,
            optimize: /status · OK/,
            check: /status · OK/,
          },
          gone: { table: GONE, drop: () => sh(DB, `DROP TABLE ${GONE};`) },
          missing:
            /doesn't exist|does not exist|Unknown table|Operation failed/i,
        }),
      async () => {
        // A read-only connection: refused, and the table is not rebuilt.
        const created = () =>
          sh(
            DB,
            `SELECT create_time FROM information_schema.tables WHERE table_schema = 'platypus' AND table_name = '${RO_TABLE}';`,
          );
        const before = created();
        await expand(page, RO_NAME, DB);
        await refusedReadOnly(page, {
          path: [RO_NAME, "platypus", "Tables", RO_TABLE],
          table: RO_TABLE,
          operation: "optimize",
        });
        assert.equal(
          created(),
          before,
          "OPTIMIZE rebuilt the table of a read-only connection",
        );
      },
    );
  } finally {
    sh(DB, `DROP TABLE IF EXISTS ${TABLE}, ${GONE}, ${RO_TABLE};`);
  }
}
