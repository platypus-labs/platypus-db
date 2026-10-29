// Group J against the dev MariaDB, with performance_schema on (docker/compose.yml): as
// admin-statistics-mysql.mjs, without the sample of a statement's run MySQL 8 keeps.
import { connections, needs, sh } from "../qa-grid.mjs";
import { mysqlFlow, mysqlSetup } from "./admin-statistics-mysql.mjs";

const DB = "mariadb";
const NAME = "QA J MariaDB";

export const skip = needs(DB);

export function setup({ dir }) {
  mysqlSetup(DB);
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  try {
    await mysqlFlow(page, DB, NAME);
  } finally {
    sh(DB, "DROP TABLE IF EXISTS e2e_admin_stats;");
  }
}
