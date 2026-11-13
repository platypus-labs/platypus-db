// Bug NAV: two objects of different kinds with the same name in one schema share a navigator
// row key, so the tree treats them as one: on MySQL a function and a procedure both named
// `qa_nav_same` (or a table and an event both named `qa_nav_ev`). Clicking one selects both,
// and the second row's context menu is the multi-selection menu (Generate DDL, Copy name, Copy
// qualified name only): its details cannot be opened. React also warns about duplicate keys.
//
// Cause: `objectRows` in apps/desktop/src/components/Navigator.tsx builds the key from the
// schema and name only (`base/schema/name`), without the kind (triggers add their table).
//
// Steps: expand qa_nav_same_db, Functions and Procedures; right-click the procedure qa_nav_same.
// Expected: its own menu with Open details. Actual: the multi-selection menu without it.
import assert from "node:assert/strict";
import { DBS, childRow, connectTo, needs, openMenu, row, rows, saveConnections } from "../navigator-helpers.mjs";

export const skip = needs("mysql");

const DB = "qa_nav_same_db";

export function setup({ dir }) {
  DBS.mysql.exec(`DROP DATABASE IF EXISTS ${DB}; CREATE DATABASE ${DB}; GRANT ALL ON ${DB}.* TO 'platypus'@'%';`);
  DBS.mysql.execUser(`USE ${DB};
    CREATE FUNCTION qa_nav_same(x int) RETURNS int DETERMINISTIC RETURN x + 1;
    CREATE PROCEDURE qa_nav_same(IN x int) SELECT x;`);
  saveConnections(dir, DBS.mysql.connection("QA MY", "00000000-0000-4000-8000-0000000a0022").replace('database = "platypus"', `database = "${DB}"`));
}

export default async function (page) {
  try {
    await connectTo(page, "QA MY", "platypus");
    await page.waitFor(`${childRow(DB, "Procedures")} !== undefined`, 20_000);
    await page.eval(`${childRow(DB, "Functions")}.querySelector('.twisty').click()`);
    await page.waitFor(`${childRow(DB, "Functions")}.getAttribute('aria-expanded') === 'true'`);
    await page.eval(`${childRow(DB, "Procedures")}.querySelector('.twisty').click()`);
    await page.waitFor(`${rows("qa_nav_same")}.length === 2`);

    // A click on the function, then a right-click on the procedure (the second row).
    await page.eval(`${rows("qa_nav_same")}[0].click()`);
    await page.waitFor(`document.querySelectorAll('.tree-row.selected').length > 0`);
    const selected = await page.eval(`document.querySelectorAll('.tree-row.selected').length`);
    const items = await openMenu(page, "qa_nav_same", 1);
    assert.ok(items.includes("Open details"), `procedure menu: ${items.join(", ")}`);
    assert.equal(selected, 1, "a click on the function selects one row");
    assert.ok(await page.eval(`${row("qa_nav_same")} !== undefined`));
  } finally {
    DBS.mysql.exec(`DROP DATABASE IF EXISTS ${DB};`);
  }
}
