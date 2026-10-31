// QA5-NAV-4 (medium). "Generate DDL" of a PostgreSQL table with a `serial` column writes
// `id integer DEFAULT nextval('s.t_id_seq'::regclass) NOT NULL` and no sequence: run in a
// database that lacks the sequence (the point of a DDL script) it fails with
// `relation "s.t_id_seq" does not exist`. serial is the commonest key of PostgreSQL schemas.
// Steps: CREATE TABLE t (id serial PRIMARY KEY); right-click t > Generate DDL.
// Expected: `serial`, an IDENTITY column or a CREATE SEQUENCE ... OWNED BY before the table.
// Actual: nextval() on a sequence the script does not create.
// Code: crates/platypus-driver-pg/src/ddl.rs `column_line` (the default is printed as is; no
//   dependency on the owned sequence).
import assert from "node:assert/strict";
import { pgDdl } from "../qa5-nav-pg-ddl.mjs";

const s = pgDdl({
  schema: "qa5_ser",
  id: "02",
  table: "t",
  sql: `CREATE TABLE qa5_ser.t (id serial PRIMARY KEY, v int);`,
  check: (ddl) => {
    if (/nextval\(/i.test(ddl)) assert.match(ddl, /CREATE SEQUENCE/i, `nextval() without a CREATE SEQUENCE:\n${ddl}`);
  },
});
export const skip = s.skip;
export const setup = s.setup;
export default s.run;
