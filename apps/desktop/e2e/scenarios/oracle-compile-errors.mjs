// Bug ORA-04: a CREATE PROCEDURE / FUNCTION / PACKAGE / TRIGGER that does not compile is
// reported as a success ("0 rows affected"): Oracle creates the object INVALID and answers
// "success with compilation error" (ORA-24344, a warning), which the driver drops
// (crates/platypus-driver-oracle/src/session.rs, Session::execute). The user finds out only
// when calling it (PLS-00905 object is invalid); SHOW ERRORS is not available either.
// Steps: run, as a script, CREATE OR REPLACE PROCEDURE QA_ORA_BAD AS BEGIN x := 1; END; /
// Expected: an error or warning naming the compilation errors (DBeaver and SQL Developer show
// "PLS-00201: identifier 'X' must be declared" at line 3). Actual: "0 rows affected", no
// message.
import { assert, dropAll, needsOracle, oracleEditor, oracleSetup, runOra, visibleTab } from "../oracle-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("PROCEDURE QA_ORA_BAD");
}

export default async function (page) {
  try {
    await oracleEditor(page);
    const { error } = await runOra(page, "CREATE OR REPLACE PROCEDURE QA_ORA_BAD AS\nBEGIN\n  x := 1;\nEND;\n/", true);
    const shown = error ?? (await page.eval(`${visibleTab}.textContent`));
    assert.match(shown, /PLS-00201|compil/i, "a procedure that does not compile is reported as a success");
  } finally {
    dropAll("PROCEDURE QA_ORA_BAD");
  }
}
