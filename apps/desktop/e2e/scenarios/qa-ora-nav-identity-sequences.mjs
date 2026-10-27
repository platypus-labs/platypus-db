// Bug ORA-25 (low): the Sequences folder of an Oracle schema lists the system-generated
// sequences behind identity columns (ISEQ$$_74026…), one per identity column, including those
// of dropped tables still in the recycle bin. They cannot be altered or dropped by the user
// (ORA-32794) and clutter the list; DBeaver and SQL Developer hide them. The objects query keeps
// every SEQUENCE of ALL_OBJECTS (crates/platypus-driver-oracle/src/meta.rs, objects_sql);
// ALL_OBJECTS.GENERATED = 'Y' or the names in ALL_TAB_IDENTITY_COLS tell them apart.
// Steps: CREATE TABLE QA_ORA_IS (ID NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY) and a
// sequence QA_ORA_IS_SEQ; expand PLATYPUS › Sequences.
// Expected: QA_ORA_IS_SEQ only. Actual: also ISEQ$$_… entries.
import { assert, connectOracle, dropAll, needsOracle, ora, oracleSetup } from "../qa-ora.mjs";
import { row, treeLabels } from "../qa-nav-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_IS PURGE", "SEQUENCE QA_ORA_IS_SEQ");
  ora("CREATE TABLE QA_ORA_IS (ID NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY);\nCREATE SEQUENCE QA_ORA_IS_SEQ;");
}

export default async function (page) {
  try {
    await connectOracle(page, "Sequences");
    if (await page.eval(`${row("Sequences")}.getAttribute('aria-expanded') !== 'true'`)) await page.eval(`${row("Sequences")}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("QA_ORA_IS_SEQ")} !== undefined`, 20_000);
    const labels = await page.eval(treeLabels);
    const generated = labels.filter((l) => /^ISEQ\$\$_/.test(l ?? ""));
    assert.deepEqual(generated, [], `system sequences listed: ${generated.join(", ")}`);
  } finally {
    dropAll("TABLE QA_ORA_IS PURGE", "SEQUENCE QA_ORA_IS_SEQ");
  }
}
