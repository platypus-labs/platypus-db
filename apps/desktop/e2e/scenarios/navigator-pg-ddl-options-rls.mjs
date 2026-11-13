// QA5-NAV-6 (low). "Generate DDL" of a PostgreSQL table leaves out what is not columns,
// constraints or indexes: storage parameters (`WITH (fillfactor=70)`), row level security
// (`ENABLE ROW LEVEL SECURITY`) and its policies, and the comments of indexes. A table copied by
// its DDL loses its security: the policy that hid rows is not there, and the table is open.
// Steps: CREATE TABLE t (...) WITH (fillfactor=70); ENABLE ROW LEVEL SECURITY; CREATE POLICY;
//   right-click t > Generate DDL.
// Expected: the script has WITH (fillfactor...), ENABLE ROW LEVEL SECURITY and CREATE POLICY.
// Actual: none of them.
// Code: crates/platypus-driver-pg/src/ddl.rs `create_table` / `table_rows` (relkind, partition
//   key and comments only).
import assert from "node:assert/strict";
import { pgDdl } from "../navigator-pg-ddl.mjs";

const s = pgDdl({
  schema: "qa5_rls",
  id: "04",
  table: "t",
  sql: `CREATE TABLE qa5_rls.t (id int PRIMARY KEY, owner text) WITH (fillfactor=70);
        ALTER TABLE qa5_rls.t ENABLE ROW LEVEL SECURITY;
        CREATE POLICY p_owner ON qa5_rls.t FOR SELECT USING (owner = current_user);`,
  check: (ddl) => {
    const missing = [/fillfactor/i, /ROW LEVEL SECURITY/i, /CREATE POLICY/i].filter((re) => !re.test(ddl));
    assert.deepEqual(missing.map(String), [], `missing from the DDL:\n${ddl}`);
  },
});
export const skip = s.skip;
export const setup = s.setup;
export default s.run;
