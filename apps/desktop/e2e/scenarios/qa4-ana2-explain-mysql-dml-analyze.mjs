// qa4-ana2-6: Explain analyze of an UPDATE or DELETE on MySQL shows the server's error text as
// the one "operation" of a plan.
// Steps: MySQL 8.4, table t (InnoDB, 20000 rows); editor: UPDATE t SET kind = 'z' WHERE id < 100;
//   context menu > Explain analyze.
// Expected: a notice that MySQL cannot analyze a change (estimates only / use Explain plan),
//   or the estimated plan of the statement; never an error text drawn as a plan node.
// Actual: the tree holds one step named "<not executable by iterator executor>" with that text
//   as its Operation, no cost, no rows: it reads as a plan, it is MySQL's message. The data is
//   untouched (checked).
// Code: crates/platypus-driver-mysql explain (EXPLAIN ANALYZE output taken as the tree whatever
//   it says); apps/desktop/src/explain/ExplainTab.tsx has no case for it.
import assert from "node:assert/strict";
import { sh } from "../qa-grid.mjs";
import { editorMenu, openEditor, saveConnections, setEditor, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("mysql");
export function setup({ dir }) {
  saveConnections(dir, ["mysql"]);
  sh("mysql", `DROP TABLE IF EXISTS qa4a2_dml; CREATE TABLE qa4a2_dml (id INT PRIMARY KEY, kind VARCHAR(10)) ENGINE=InnoDB;
    INSERT INTO qa4a2_dml VALUES (1, 'a'), (2, 'b'), (3, 'c');`);
}
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;

export default async function (page) {
  try {
    await openEditor(page, "QA MySQL");
    await setEditor(page, "UPDATE qa4a2_dml SET kind = 'z' WHERE id < 3");
    await editorMenu(page, "Explain analyze");
    await page.waitFor(`${explainTab} !== undefined && !${explainTab}.textContent.includes('Running')`, 20_000);
    await page.sleep(1500);
    const text = await page.eval(`${explainTab}.innerText`);
    assert.equal(sh("mysql", "SELECT count(*) FROM qa4a2_dml WHERE kind = 'z'"), "0", "the data stays as it was");
    assert.doesNotMatch(text, /not executable by iterator executor/i, text.slice(0, 400));
  } finally {
    sh("mysql", "DROP TABLE IF EXISTS qa4a2_dml;");
  }
}
