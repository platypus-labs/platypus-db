// Group J against the dev MySQL: Server activity's costliest statements (from
// performance_schema's digests) and tables and indexes, with ANALYZE, OPTIMIZE and CHECK
// TABLE from the table's details and ANALYZE from the navigator, whose dialog shows the
// server's status rows. See statisticsFlow in e2e/admin.mjs.
import { connections, needs, sh } from "../qa-grid.mjs";
import { statisticsFlow } from "../admin.mjs";

const DB = "mysql";
const NAME = "QA J MySQL";
const TABLE = "e2e_admin_stats";

export const skip = needs(DB);

export function mysqlSetup(db) {
  sh(
    db,
    `DROP TABLE IF EXISTS ${TABLE};
     CREATE TABLE ${TABLE} (id int PRIMARY KEY, v int);
     CREATE INDEX ${TABLE}_v ON ${TABLE} (v);
     INSERT INTO ${TABLE} VALUES (1, 1), (2, 2), (3, 3);
     SELECT SLEEP(1) FROM ${TABLE} WHERE id = 1;
     SELECT SLEEP(1) FROM ${TABLE} WHERE id = 2;`,
  );
}

export const mysqlFlow = (page, db, name) =>
  statisticsFlow(page, {
    db,
    name,
    table: TABLE,
    slow: TABLE,
    path: [name, "platypus", "Tables", TABLE],
    operations: ["analyze", "optimize", "check"],
    output: /status · OK/,
  });

export function setup({ dir }) {
  mysqlSetup(DB);
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  try {
    await mysqlFlow(page, DB, NAME);
  } finally {
    sh(DB, `DROP TABLE IF EXISTS ${TABLE};`);
  }
}
