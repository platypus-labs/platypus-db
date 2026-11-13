// Group J against the dev PostgreSQL: Server activity's costliest statements (from
// pg_stat_statements, preloaded by docker/compose.yml) and tables and indexes, with
// VACUUM, ANALYZE and REINDEX from the table's details and VACUUM from the navigator,
// whose dialog shows VACUUM VERBOSE's output. See statisticsFlow in e2e/admin.mjs.
import { connections, needs, sh } from "../grid-helpers.mjs";
import { statisticsFlow } from "../admin.mjs";

const DB = "pg";
const NAME = "QA J PostgreSQL";
const TABLE = "e2e_admin_stats";

export const skip = needs(DB);

export function setup({ dir }) {
  sh(
    DB,
    `CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
     DROP TABLE IF EXISTS demo.${TABLE};
     CREATE TABLE demo.${TABLE} (id int PRIMARY KEY, v int);
     CREATE INDEX ${TABLE}_v ON demo.${TABLE} (v);
     INSERT INTO demo.${TABLE} SELECT g, g FROM generate_series(1, 1000) g;
     SELECT pg_stat_statements_reset();
     SELECT pg_sleep(0.5) FROM demo.${TABLE} WHERE id = 1;
     SELECT pg_sleep(0.5) FROM demo.${TABLE} WHERE id = 2;`,
  );
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  try {
    await statisticsFlow(page, {
      db: DB,
      name: NAME,
      table: TABLE,
      slow: `FROM demo.${TABLE} WHERE id =`,
      path: [NAME, "platypus", "demo", "Tables", TABLE],
      operations: ["vacuum", "analyze", "reindex"],
      output: /vacuuming/i,
    });
  } finally {
    sh(DB, `DROP TABLE IF EXISTS demo.${TABLE};`);
  }
}
