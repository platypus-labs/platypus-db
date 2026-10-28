// SCH-04 (high). Compare with… copies a serial column's default as it is, sequence schema
// included: DEFAULT nextval('qa3s_src.customers_id_seq'::regclass). Against another database
// (a second connection) the script fails, the sequence is not there; against another schema of
// the same database it runs, and the target table then draws its ids from the SOURCE schema's
// sequence (dropping qa3s_src later breaks or cascades into qa3s_dst).
// Steps: qa3s_src.customers (id serial PRIMARY KEY, name text); a second connection to the
// database qa3s_db2 with an empty schema qa3s_dst; Compare with… > QA3 PG db2 · qa3s_dst;
// run the script; compare again. Then the same against qa3s_dst of the same database.
// Expected: the script creates the column as serial (or its own sequence in the target schema),
// never names qa3s_src, runs, and the second comparison finds nothing to change.
// Actual: "relation "qa3s_src.customers_id_seq" does not exist" on the other database; on the
// same database the target's default points to qa3s_src.customers_id_seq.
// Suspected: crates/platypus-schema/src/compare.rs:84-133 (matched rewrites the schema of the
// foreign keys only, not of defaults) and the PostgreSQL design read by crates/platypus-schema
// load (serial columns are not recognised as such).
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, needsEngines, openPath, psql, roundTrip, saveEngines, told } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");
const DB2 = "qa3s_db2";

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.customers (id serial PRIMARY KEY, name text);`);
  ENGINES.pg.seed(dir, DST, "");
  psql(`DROP DATABASE IF EXISTS ${DB2} WITH (FORCE);`);
  psql(`CREATE DATABASE ${DB2};`);
  psql(`CREATE SCHEMA ${DST};`, DB2);
  saveEngines(dir, ["pg"], [
    {
      id: "00000000-0000-4000-8000-0000000c3102",
      name: "QA3 PG db2",
      body: `driver = "postgres"\nhost = "127.0.0.1"\nport = 55432\ndatabase = "${DB2}"\nuser = "platypus"\nssl_mode = "disable"\npassword_command = "echo platypus"`,
    },
  ]);
}

export default async function (page) {
  const problems = [];
  try {
    await openPath(page, ["QA3 PG", "platypus", SRC]);
    // Another database.
    const other = await roundTrip(page, ["QA3 PG", "platypus"], SRC, "QA3 PG db2", DST);
    if (!other.sql) problems.push(`no script for another database: ${told(other)}`);
    else {
      if (/qa3s_src/.test(other.sql)) problems.push(`the script for another database names the source schema:
${other.sql}`);
      if (other.runError !== null) problems.push(`the script fails on another database: ${other.runError}`);
      else if (other.second.same !== true) problems.push(`a second comparison still finds changes: ${told(other)}`);
    }
    // Same database, another schema: the table must not take the source's sequence.
    const same = await roundTrip(page, ["QA3 PG", "platypus"], SRC, "QA3 PG", DST);
    if (!same.sql || same.runError !== null) problems.push(`same database: ${told(same)}`);
    const def = psql(`SELECT column_default FROM information_schema.columns WHERE table_schema = '${DST}' AND table_name = 'customers' AND column_name = 'id'`);
    if (/qa3s_src/.test(def)) problems.push(`qa3s_dst.customers.id takes its values from the source schema: ${def}`);
  } finally {
    ENGINES.pg.drop(DST);
    ENGINES.pg.drop(SRC);
    psql(`DROP DATABASE IF EXISTS ${DB2} WITH (FORCE);`);
  }
  assert.deepEqual(problems, []);
}
