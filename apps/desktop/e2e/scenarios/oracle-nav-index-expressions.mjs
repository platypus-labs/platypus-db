// Bug ORA-10: the Indexes of an Oracle table show hidden system column names instead of the
// indexed expressions: a function-based index on UPPER(NAME) lists "SYS_NC00003$", and a
// descending column (SALARY DESC, also stored as an expression) lists "SYS_NC00004$". The
// index query reads only ALL_IND_COLUMNS.COLUMN_NAME (crates/platypus-driver-oracle/src/
// meta.rs, indexes_sql); the expression is in ALL_IND_EXPRESSIONS.COLUMN_EXPRESSION.
// Steps: QA_ORA_IX (ID PK, NAME, SALARY) with indexes on (UPPER(NAME)) and (ID, SALARY DESC);
// Open details › Indexes.
// Expected: UPPER("NAME") and ID, SALARY DESC (as DBeaver shows). Actual: SYS_NC…$ names.
import { assert, connectOracle, dropAll, needsOracle, ora, oracleSetup } from "../oracle-helpers.mjs";
import { menu, row } from "../navigator-helpers.mjs";

export const skip = needsOracle();
const objTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_IX");
  ora(`CREATE TABLE QA_ORA_IX (ID NUMBER PRIMARY KEY, NAME VARCHAR2(20), SALARY NUMBER);
CREATE INDEX QA_ORA_IX_UPPER ON QA_ORA_IX (UPPER(NAME));
CREATE INDEX QA_ORA_IX_DESC ON QA_ORA_IX (ID, SALARY DESC);`);
}

export default async function (page) {
  try {
    await connectOracle(page, "Tables");
    await page.waitFor(`${row("QA_ORA_IX")} !== undefined`, 20_000);
    await menu(page, "QA_ORA_IX", "Open details");
    await page.waitFor(`${objTab}?.querySelector('.object-sections') != null`, 15_000);
    await page.eval(`[...${objTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Indexes')).click()`);
    await page.waitFor(`(${objTab}.querySelector('.object-body')?.innerText ?? '').includes('QA_ORA_IX_UPPER')`, 15_000);
    const text = await page.eval(`${objTab}.querySelector('.object-body').innerText`);
    assert.doesNotMatch(text, /SYS_NC\d+\$/, text);
    assert.match(text, /UPPER\("?NAME"?\)/, text);
  } finally {
    dropAll("TABLE QA_ORA_IX");
  }
}
