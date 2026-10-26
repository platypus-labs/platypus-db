// Bug NAV: on MySQL, the table editor cannot change a generated column. Editing anything of
// `dbl int GENERATED ALWAYS AS (qty * 2) VIRTUAL` (here its comment) writes
// `CHANGE COLUMN dbl dbl int VIRTUAL GENERATED COMMENT 'doubled'`: MySQL's EXTRA text
// (`VIRTUAL GENERATED`) is pasted as if it were syntax and the generation expression is not
// in the design at all, so the statement fails with error 1064 (and, were it accepted, the
// column would lose its expression).
//
// Cause: `Ddl::column` in crates/platypus-schema/src/ddl.rs appends `extra` verbatim for
// MySQL; the design keeps no generation expression (ColumnDesign in
// crates/platypus-schema/src/lib.rs).
//
// Steps: Edit table… on qa_nav_g, type a comment for dbl, Save…, Apply.
// Expected: the comment saved, the column still `GENERATED ALWAYS AS ((qty * 2)) VIRTUAL`.
// Actual: 1064 syntax error near 'VIRTUAL GENERATED'.
import assert from "node:assert/strict";
import { DBS, connectTo, menu, needs, row, saveConnections } from "../qa-nav-helpers.mjs";
import { cell, confirm, editor, fill, openSave } from "../qa-nav-table-editor.mjs";

export const skip = needs("mysql");

const DB = "qa_nav_gen";

export function setup({ dir }) {
  DBS.mysql.exec(`DROP DATABASE IF EXISTS ${DB}; CREATE DATABASE ${DB}; GRANT ALL ON ${DB}.* TO 'platypus'@'%';
    CREATE TABLE ${DB}.qa_nav_g (id int PRIMARY KEY, qty int, dbl int GENERATED ALWAYS AS (qty * 2) VIRTUAL);`);
  saveConnections(dir, DBS.mysql.connection("QA MY", "00000000-0000-4000-8000-0000000a0023").replace('database = "platypus"', `database = "${DB}"`));
}

export default async function (page) {
  try {
    await connectTo(page, "QA MY", "platypus");
    await page.waitFor(`${row("qa_nav_g")} !== undefined`, 20_000);
    await menu(page, "qa_nav_g", "Edit table…");
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 3`, 10_000);
    await fill(page, cell(2, 5), "doubled");
    const sql = await openSave(page);
    const error = await confirm(page);
    assert.equal(error, null, `script:\n${sql}`);
    const ddl = DBS.mysql.exec(`SHOW CREATE TABLE ${DB}.qa_nav_g`);
    assert.match(ddl, /`dbl` int GENERATED ALWAYS AS \(\(`qty` \* 2\)\) VIRTUAL COMMENT 'doubled'/);
  } finally {
    DBS.mysql.exec(`DROP DATABASE IF EXISTS ${DB};`);
  }
}
