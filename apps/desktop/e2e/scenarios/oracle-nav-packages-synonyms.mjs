// Bug ORA-09: the Oracle navigator lists no packages (nor their bodies) and no synonyms: the
// objects query keeps only TABLE, VIEW, MATERIALIZED VIEW, SEQUENCE, FUNCTION, PROCEDURE,
// TRIGGER and TYPE (crates/platypus-driver-oracle/src/meta.rs, OBJECT_TYPES). Packages hold
// most PL/SQL code in real schemas; DBeaver and SQL Developer show Packages (spec and body,
// with their DDL) and Synonyms folders.
// Steps: CREATE PACKAGE QA_ORA_NPKG (+ BODY) and CREATE SYNONYM QA_ORA_NSYN FOR a table;
// connect; look at the PLATYPUS schema.
// Expected: the package and the synonym in the tree. Actual: neither is listed.
import { assert, connectOracle, dropAll, needsOracle, ora, oracleSetup } from "../oracle-helpers.mjs";
import { row, treeLabels } from "../navigator-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("PACKAGE QA_ORA_NPKG", "SYNONYM QA_ORA_NSYN", "TABLE QA_ORA_NTAB");
  ora(`CREATE TABLE QA_ORA_NTAB (ID NUMBER PRIMARY KEY);
CREATE SYNONYM QA_ORA_NSYN FOR QA_ORA_NTAB;
CREATE OR REPLACE PACKAGE QA_ORA_NPKG AS
  FUNCTION one RETURN NUMBER;
END;
/
CREATE OR REPLACE PACKAGE BODY QA_ORA_NPKG AS
  FUNCTION one RETURN NUMBER IS BEGIN RETURN 1; END;
END;
/`);
}

export default async function (page) {
  try {
    await connectOracle(page, "Tables");
    await page.waitFor(`${row("QA_ORA_NTAB")} !== undefined`, 20_000);
    // Expand every folder of the schema.
    const folders = await page.eval(`[...document.querySelectorAll('.tree-row')].filter(r => r.getAttribute('aria-level') === '3' && r.getAttribute('aria-expanded') === 'false').map(r => r.querySelector('.tree-label').textContent)`);
    for (const f of folders) {
      await page.eval(`${row(f)}.querySelector('.twisty').click()`);
      await page.sleep(500);
    }
    const labels = await page.eval(treeLabels);
    assert.ok(labels.includes("QA_ORA_NPKG"), `no package in the tree: ${labels.slice(0, 40).join(", ")}`);
    assert.ok(labels.includes("QA_ORA_NSYN"), "no synonym in the tree");
  } finally {
    dropAll("PACKAGE QA_ORA_NPKG", "SYNONYM QA_ORA_NSYN", "TABLE QA_ORA_NTAB");
  }
}
