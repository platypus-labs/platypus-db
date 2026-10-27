// Bug ORA-20: importing into an Oracle table with "Update rows with the same key, add the
// others" fails with "Nothing was imported: ClickHouse has no upsert". The mode is offered for
// Oracle, the message names the wrong database, and Oracle has MERGE for exactly this (the
// grid's "Generate UPSERT" is not offered for Oracle either). Suspected: the upsert of
// crates/platypus-sql/src/generate.rs returns None for Dialect::Oracle and the import reports
// that with a ClickHouse-only text (crates/platypus-transfer).
// Steps: QA_ORA_IU (ID NUMBER(10) PK, NAME VARCHAR2(20)) with (1, 'a'), (2, 'b'); Import data…
// from a CSV with rows 2,B and 3,c; mode "Update rows with the same key, add the others".
// Expected: 1|a, 2|B, 3|c (or no such mode for Oracle). Actual: the ClickHouse error, nothing
// imported.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { assert, CONN, dropAll, needsOracle, ora, oracleSetup } from "../qa-ora.mjs";
import { chooseFile, closeModal, importDialog, menu, runJob, showTable } from "../qa-xfer.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_IU");
  ora(`CREATE TABLE QA_ORA_IU (ID NUMBER(10) PRIMARY KEY, NAME VARCHAR2(20));
INSERT INTO QA_ORA_IU VALUES (1, 'a');
INSERT INTO QA_ORA_IU VALUES (2, 'b');
COMMIT;`);
  writeFileSync(join(dir, "upd.csv"), "ID,NAME\n2,B\n3,c\n");
}

export default async function (page, { dir }) {
  try {
    await showTable(page, CONN, "PLATYPUS", "QA_ORA_IU");
    await menu(page, "QA_ORA_IU", "Import data", importDialog, [CONN, "PLATYPUS"]);
    assert.equal(await chooseFile(page, join(dir, "upd.csv")), null);
    await page.waitFor(`${importDialog}.querySelectorAll('.import-mapping select').length === 2`, 15_000);
    const offered = await page.eval(`[...${importDialog}.querySelectorAll('.import-modes label')].some(l => l.textContent.startsWith('Update'))`);
    if (offered) {
      await page.eval(`[...${importDialog}.querySelectorAll('.import-modes label')].find(l => l.textContent.startsWith('Update')).querySelector('input').click()`);
      const out = await runJob(page, importDialog, "Import", 60_000);
      await closeModal(page, importDialog);
      assert.doesNotMatch(out, /ClickHouse/, out);
      assert.match(out, /Imported 2 rows/, out);
      assert.equal(ora("SELECT LISTAGG(ID || '|' || NAME, ',') WITHIN GROUP (ORDER BY ID) FROM QA_ORA_IU;"), "1|a,2|B,3|c");
    }
  } finally {
    dropAll("TABLE QA_ORA_IU");
  }
}
