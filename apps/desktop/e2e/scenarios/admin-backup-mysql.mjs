// Group J against the dev MySQL: Back up… writes a mysqldump file of the database, table by
// table, and Restore… runs it through mysql, asked first, bringing a dropped table back.
// Needs mysqldump and mysql on the PATH.
import { connections, needs, sh } from "../grid-helpers.mjs";
import { backupFlow, toolVersion } from "../admin.mjs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const DB = "mysql";
const NAME = "QA J MySQL";
const TABLE = "e2e_backup";

export const skip = async () =>
  (await needs(DB)()) || (toolVersion("mysqldump") === null || toolVersion("mysql") === null ? "mysqldump or mysql is not on the PATH" : null);

/**
 * The seed view is made by root, whose SYSTEM_USER privilege keeps `platypus` from dropping it: the
 * DROP VIEW of the restore fails at the first statement. Re-made as `platypus` it restores like
 * any other object (the same SELECT as docker/mysql/01-seed.sql).
 */
function ownSeedView() {
  const sql = `DROP VIEW IF EXISTS customer_totals; CREATE DEFINER = 'platypus'@'%' SQL SECURITY INVOKER VIEW customer_totals AS SELECT c.id, c.full_name, COUNT(o.id) AS orders, COALESCE(SUM(o.total), 0) AS spent FROM customers c LEFT JOIN orders o ON o.customer_id = c.id GROUP BY c.id, c.full_name;`;
  const r = spawnSync("docker", ["exec", "-i", "platypus-db-mysql-1", "mysql", "-uroot", "-pplatypus", "platypus"], { input: sql, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`mysql: ${r.stderr}`);
}

export function setup({ dir }) {
  ownSeedView();
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
