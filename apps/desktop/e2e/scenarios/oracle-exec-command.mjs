// Bug ORA-26 (low): the SQL*Plus shorthand "EXEC proc(args)" fails on Oracle with ORA-00900
// "invalid SQL statement"; only CALL or BEGIN … END; work. EXEC[UTE] is the usual way to call
// a procedure in scripts copied from SQL*Plus / SQL Developer, and DBeaver runs it (as
// BEGIN … END;). SHOW ERRORS, the companion of CREATE PROCEDURE, fails the same way (see
// oracle-compile-errors for the missing compilation errors).
// Steps: run EXEC DBMS_OUTPUT.PUT_LINE('x').
// Expected: runs like BEGIN DBMS_OUTPUT.PUT_LINE('x'); END;. Actual: ORA-00900.
import { assert, needsOracle, oracleEditor, oracleSetup, runOra } from "../oracle-helpers.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

export default async function (page) {
  await oracleEditor(page);
  const { error } = await runOra(page, "EXEC DBMS_OUTPUT.PUT_LINE('x')");
  assert.equal(error, null);
}
