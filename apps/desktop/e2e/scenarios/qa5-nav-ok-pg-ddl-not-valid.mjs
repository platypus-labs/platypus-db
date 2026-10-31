// Regression guard: "Generate DDL" of a PostgreSQL table keeps a `NOT VALID` constraint (added
// to a table with old rows that break it), so the script recreates the same state.
import assert from "node:assert/strict";
import { pgDdl } from "../qa5-nav-pg-ddl.mjs";

const s = pgDdl({
  schema: "qa5_nv",
  id: "03",
  table: "t",
  sql: `CREATE TABLE qa5_nv.t (n int); INSERT INTO qa5_nv.t VALUES (-1); ALTER TABLE qa5_nv.t ADD CONSTRAINT nn CHECK (n > 0) NOT VALID;`,
  check: (ddl) => assert.match(ddl, /NOT VALID/, `NOT VALID is gone:\n${ddl}`),
});
export const skip = s.skip;
export const setup = s.setup;
export default s.run;
