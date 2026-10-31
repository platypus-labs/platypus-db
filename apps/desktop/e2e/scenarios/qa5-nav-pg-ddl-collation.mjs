// QA5-NAV-3 (medium). "Generate DDL" of a PostgreSQL table drops the COLLATE of its columns:
// `note text COLLATE "C"` and `name varchar(20) COLLATE "und-x-icu"` come out as plain `text` and
// `character varying(20)`. Recreated from the script (or copied to another database) the
// columns sort and compare differently, and a unique index on them can change meaning.
// Steps: CREATE TABLE t (note text COLLATE "C", ...); right-click t > Generate DDL.
// Expected: `note text COLLATE "C"`. Actual: `note text`.
// Code: crates/platypus-driver-pg/src/ddl.rs `table_rows` (the columns query reads no
//   attcollation) and `column_line`.
import assert from "node:assert/strict";
import { pgDdl } from "../qa5-nav-pg-ddl.mjs";

const s = pgDdl({
  schema: "qa5_coll",
  id: "01",
  table: "t",
  sql: `CREATE TABLE qa5_coll.t (id int PRIMARY KEY, note text COLLATE "C", name varchar(20) COLLATE "und-x-icu");`,
  check: (ddl) => assert.match(ddl, /COLLATE\s+"?C"?/, `the collation of note is gone:\n${ddl}`),
});
export const skip = s.skip;
export const setup = s.setup;
export default s.run;
