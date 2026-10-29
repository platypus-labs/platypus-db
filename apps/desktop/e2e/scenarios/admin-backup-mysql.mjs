// Group J against the dev MySQL: Back up… writes a mysqldump file of the database, table by
// table, and Restore… runs it through mysql, asked first, bringing a dropped table back.
// Needs mysqldump and mysql on the PATH.
import { connections, needs, sh } from "../qa-grid.mjs";
import { backupFlow, toolVersion } from "../admin.mjs";
import { join } from "node:path";

const DB = "mysql";
const NAME = "QA J MySQL";
const TABLE = "e2e_backup";

export const skip = async () =>
  (await needs(DB)()) || (toolVersion("mysqldump") === null || toolVersion("mysql") === null ? "mysqldump or mysql is not on the PATH" : null);

export function setup({ dir }) {
  sh(DB, `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, v text); INSERT INTO ${TABLE} VALUES (1, 'one'), (2, 'two');`);
  connections(dir, [[NAME, DB]]);
}

export default async function (page, { dir }) {
  try {
    await backupFlow(page, {
      db: DB,
      name: NAME,
      file: join(dir, "platypus.sql"),
      table: TABLE,
      dropTable: () => sh(DB, `DROP TABLE ${TABLE}`),
      count: () => {
        const n = sh(DB, `SELECT count(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA = 'platypus' AND TABLE_NAME = '${TABLE}'`);
        return n === "0" ? null : Number(sh(DB, `SELECT count(*) FROM ${TABLE}`));
      },
    });
  } finally {
    sh(DB, `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
