// Group J against the dev PostgreSQL: Back up… writes a pg_dump archive of the database,
// table by table, and Restore… brings a dropped table back from it with pg_restore, asked
// first. Needs pg_dump and pg_restore 17 or later on the PATH (the server's version).
import { connections, needs, sh } from "../grid-helpers.mjs";
import { backupFlow, toolVersion } from "../admin.mjs";
import { join } from "node:path";

const DB = "pg";
const NAME = "QA J PostgreSQL";
const TABLE = "demo.e2e_backup";

export const skip = async () =>
  (await needs(DB)()) || ((toolVersion("pg_dump") ?? 0) < 17 ? "pg_dump 17 or later is not on the PATH" : null);

export function setup({ dir }) {
  sh(DB, `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, v text); INSERT INTO ${TABLE} VALUES (1, 'one'), (2, 'two');`);
  connections(dir, [[NAME, DB]]);
}

export default async function (page, { dir }) {
  try {
    await backupFlow(page, {
      db: DB,
      name: NAME,
      file: join(dir, "platypus.dump"),
      table: TABLE,
      clean: true,
      dropTable: () => sh(DB, `DROP TABLE ${TABLE}`),
      count: () => {
        const n = sh(DB, `SELECT count(*) FROM pg_tables WHERE schemaname || '.' || tablename = '${TABLE}'`);
        return n === "0" ? null : Number(sh(DB, `SELECT count(*) FROM ${TABLE}`));
      },
    });
  } finally {
    sh(DB, `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
