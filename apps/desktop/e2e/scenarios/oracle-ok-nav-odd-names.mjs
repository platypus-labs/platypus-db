// Regression guard (passes): Oracle tables with odd names work from the navigator: mixed case
// with a space, all lower case, a `#` and a `$`, a 60-character name; Open data reads their
// rows (with quoted mixed-case, unicode and reserved-word columns), and Copy qualified name
// quotes only what needs it.
import { assert, connectOracle, dropAll, needsOracle, ora, oracleSetup } from "../oracle-helpers.mjs";
import { gridText, menu, row, visibleTab, waitRun } from "../navigator-helpers.mjs";

export const skip = needsOracle();
const LONG = "QA_ORA_A_TABLE_NAME_LONGER_THAN_THIRTY_CHARACTERS_FOR_TESTING";
const TABLES = [
  ["QA_ORA_On Mixed", `"Id" NUMBER PRIMARY KEY, "Value 'q'" VARCHAR2(5), "select" NUMBER, "ünï" VARCHAR2(5)`, "1, 'a', 2, 'ü'", "Id\tValue 'q'\tselect\tünï\n1\ta\t2\tü"],
  ["qa_ora_on_lower", `"id" NUMBER PRIMARY KEY`, "1", "id\n1"],
  ["QA_ORA_ON#H$", "ID NUMBER PRIMARY KEY, V$X VARCHAR2(5)", "1, 'h'", "ID\tV$X\n1\th"],
  [LONG, "ID NUMBER PRIMARY KEY", "1", "ID\n1"],
];

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll(...TABLES.map(([t]) => `TABLE "${t}"`));
  ora(TABLES.map(([t, cols, values]) => `CREATE TABLE "${t}" (${cols});\nINSERT INTO "${t}" VALUES (${values});`).join("\n") + "\nCOMMIT;");
}

export default async function (page) {
  try {
    await connectOracle(page, "Tables");
    for (const [table, , , expected] of TABLES) {
      await page.waitFor(`${row(table)} !== undefined`, 20_000);
      await menu(page, table, "Open data");
      await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null && document.querySelector('.tab.active')?.textContent.startsWith(${JSON.stringify(table)})`, 15_000);
      await waitRun(page);
      assert.equal(await gridText(page), expected, table);
    }
    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }; window.__copied = undefined`);
    await menu(page, "QA_ORA_On Mixed", "Copy qualified name");
    await page.waitFor(`window.__copied !== undefined`);
    assert.equal(await page.eval(`window.__copied`), `PLATYPUS."QA_ORA_On Mixed"`);
  } finally {
    dropAll(...TABLES.map(([t]) => `TABLE "${t}"`));
  }
}
