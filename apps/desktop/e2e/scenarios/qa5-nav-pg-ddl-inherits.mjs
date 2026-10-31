// QA5-NAV-7 (low). "Generate DDL" of a PostgreSQL table that INHERITS another prints a
// standalone table with the parent's columns: the link to the parent (and with it the rows
// the parent's queries see) is not in the script. Partitions are handled (PARTITION OF);
// classic inheritance is not.
// Steps: CREATE TABLE parent (id int); CREATE TABLE kid (extra int) INHERITS (parent);
//   right-click kid > Generate DDL.
// Expected: `INHERITS (s.parent)`. Actual: a plain CREATE TABLE with id and extra.
// Code: crates/platypus-driver-pg/src/ddl.rs `create_table` (only relispartition parents are read).
import assert from "node:assert/strict";
import { pgDdl } from "../qa5-nav-pg-ddl.mjs";

const s = pgDdl({
  schema: "qa5_inh",
  id: "05",
  table: "kid",
  sql: `CREATE TABLE qa5_inh.parent (id int); CREATE TABLE qa5_inh.kid (extra int) INHERITS (qa5_inh.parent);`,
  check: (ddl) => assert.match(ddl, /INHERITS\s*\(/i, `no INHERITS:\n${ddl}`),
});
export const skip = s.skip;
export const setup = s.setup;
export default s.run;
