// Regression guard (passes): the table editor on SQL Server with a `]` in the table's name —
// create with a default, an index and a foreign key, then rename a column, change its type and
// default, and add a NOT NULL column with a default to a table holding a row.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, menu, needs, row, saveConnections } from "../navigator-helpers.mjs";
import { addRow, cell, check, confirm, editor, fill, openSave, section } from "../table-editor-helpers.mjs";

export const skip = needs("sqlserver");

const S = "qa_nav_ok_te";
const columns = (table) =>
  DBS.sqlserver
    .exec(`SET NOCOUNT ON; SELECT c.name + ' ' + t.name + ' ' + CAST(c.is_nullable AS varchar) FROM sys.columns c JOIN sys.types t ON t.user_type_id = c.user_type_id WHERE c.object_id = OBJECT_ID(N'${S}.${table}') ORDER BY c.column_id`)
    .trim()
    .split(/\r?\n/)
    .map((l) => l.trim());

function drop() {
  DBS.sqlserver.exec(`IF OBJECT_ID(N'${S}.[qa te]]x]') IS NOT NULL DROP TABLE ${S}.[qa te]]x];
    IF OBJECT_ID(N'${S}.qa_parent') IS NOT NULL DROP TABLE ${S}.qa_parent;
    IF SCHEMA_ID(N'${S}') IS NOT NULL EXEC(N'DROP SCHEMA ${S}');`);
}

export function setup({ dir }) {
  drop();
  DBS.sqlserver.exec(`EXEC(N'CREATE SCHEMA ${S}'); CREATE TABLE ${S}.qa_parent (id int PRIMARY KEY);`);
  saveConnections(dir, DBS.sqlserver.connection("QA MS", "00000000-0000-4000-8000-0000000a0031"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA MS", DBS.sqlserver.password);
    await page.waitFor(`${row(S)} !== undefined`, 30_000);
    await expand(page, S, "qa_parent");
    await menu(page, S, "Create table…");
    await page.waitFor(`${editor}?.querySelector('.table-name input') != null`, 10_000);
    await fill(page, `${editor}.querySelector('.table-name input')`, "qa te]x");
    await fill(page, cell(0, 1), "int");
    await addRow(page);
    await fill(page, cell(1, 0), "name");
    await fill(page, cell(1, 1), "nvarchar(50)");
    await fill(page, cell(1, 3), "N'd'");
    await addRow(page);
    await fill(page, cell(2, 0), "parent_id");
    await fill(page, cell(2, 1), "int");
    await section(page, "Indexes");
    await addRow(page);
    await fill(page, cell(0, 1), "name");
    await section(page, "Foreign keys");
    await addRow(page);
    await fill(page, cell(0, 1), "parent_id");
    await fill(page, cell(0, 3), "qa_parent");
    await fill(page, cell(0, 4), "id");
    assert.match(await openSave(page), /CREATE TABLE qa_nav_ok_te\.\[qa te\]\]x\]/);
    assert.equal(await confirm(page), null);
    assert.deepEqual(columns("[qa te]]x]"), ["id int 0", "name nvarchar 1", "parent_id int 1"]);
    DBS.sqlserver.exec(`INSERT INTO ${S}.qa_parent VALUES (1); INSERT INTO ${S}.[qa te]]x] (id, parent_id) VALUES (1, 1);`);

    // Saved: the tab now edits the table.
    await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('qa te]x')`, 15_000);
    await section(page, "Columns");
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 3 && ${editor}.querySelector('.object-header button.primary').disabled`, 15_000);
    await fill(page, cell(1, 0), "full name");
    await fill(page, cell(1, 1), "nvarchar(100)");
    await fill(page, cell(1, 3), "N'e'");
    await addRow(page);
    await fill(page, cell(3, 0), "flag");
    await fill(page, cell(3, 1), "bit");
    await check(page, cell(3, 2));
    await fill(page, cell(3, 3), "0");
    await openSave(page);
    assert.equal(await confirm(page), null);
    assert.deepEqual(columns("[qa te]]x]"), ["id int 0", "full name nvarchar 1", "parent_id int 1", "flag bit 0"]);
    const kept = DBS.sqlserver.exec(`SET NOCOUNT ON; SELECT CAST(id AS varchar) + ' ' + [full name] + ' ' + CAST(flag AS varchar) FROM ${S}.[qa te]]x]`).trim();
    assert.equal(kept, "1 d 0");
  } finally {
    drop();
  }
}
