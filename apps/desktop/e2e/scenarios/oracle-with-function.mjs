// Bug ORA-14: a query with inline PL/SQL (WITH FUNCTION …, Oracle 12c+) cannot be run: the
// splitter ends the statement at the first ";" inside the function body. Ctrl+Enter sends
// "WITH FUNCTION f(…) … RETURN x * 2" (ORA-00904 / ORA-02000), and the script runner, even
// with the "/" line SQL*Plus needs after such a query, splits it into three statements.
// (crates/platypus-sql/src/lib.rs, plsql_head: only BEGIN, DECLARE and CREATE PROCEDURE/
// FUNCTION/PACKAGE/TRIGGER/TYPE run to a "/" line; the driver's statement_text also drops
// the final ";" a WITH FUNCTION query needs inside a PL/SQL-aware client.)
// Steps: run "WITH FUNCTION f(x NUMBER) RETURN NUMBER IS BEGIN RETURN x * 2; END;
// SELECT f(21) v FROM DUAL" with Ctrl+Enter, then as a script ending with a "/" line.
// Expected: one row, V = 42, both ways. Actual: ORA-00904 "F": invalid identifier, and
// ORA-02000 with three result tabs.
import { assert, needsOracle, oracleEditor, oracleSetup, rows, runOra } from "../oracle-helpers.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

const SQL = "WITH FUNCTION f(x NUMBER) RETURN NUMBER IS BEGIN RETURN x * 2; END;\nSELECT f(21) v FROM DUAL";

export default async function (page) {
  await oracleEditor(page);
  const statement = await runOra(page, SQL);
  assert.equal(statement.error, null, `Ctrl+Enter: ${statement.error}`);
  assert.deepEqual(await rows(page), [["42"]]);
  const script = await runOra(page, `${SQL}\n/`, true);
  assert.equal(script.error, null, `script: ${script.error}`);
  assert.deepEqual(await rows(page), [["42"]]);
}
