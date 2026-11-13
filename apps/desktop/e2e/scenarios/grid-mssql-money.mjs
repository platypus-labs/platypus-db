// GRID-06 (critical: wrong data shown), SQL Server. `money` values are read as a float by the
// protocol library and then stored as a decimal with 4 places
// (crates/platypus-driver-mssql/src/columns.rs, ColumnBuilder::Money), so values with more
// than ~15 significant digits are shown rounded: 922337203685477.5807 (the largest money)
// reads 922337203685477.5808 and 98765432109876.5432 reads 98765432109876.5440. The
// rounded text is what copies, exports, filters ("Equal to this value" finds nothing) and the
// undo script of a deleted row use.
// Expected: the exact values. Actual: the last digit is off.
import { openEditor, CTRL, assert, connections, copyAll, needs, run, sh, stubClipboard, treeRow, visibleTab } from "../grid-helpers.mjs";

export const skip = needs("mssql");

export function setup({ dir }) {
  sh("mssql", `IF OBJECT_ID('dbo.qa_grid_money') IS NOT NULL DROP TABLE dbo.qa_grid_money;
    CREATE TABLE dbo.qa_grid_money (id int PRIMARY KEY, m money);
    INSERT INTO dbo.qa_grid_money VALUES (1, 922337203685477.5807), (2, -922337203685477.5808), (3, 98765432109876.5432);`);
  connections(dir, [["QA MS", "mssql"]]);
}

export default async function (page) {
  try {
    await openEditor(page, "QA MS", "mssql", "platypus");
    assert.equal(await run(page, "SELECT id, m FROM dbo.qa_grid_money ORDER BY id"), "3 rows");
    await stubClipboard(page);
    assert.equal(await copyAll(page), "1\t922337203685477.5807\n2\t-922337203685477.5808\n3\t98765432109876.5432");
  } finally {
    sh("mssql", "IF OBJECT_ID('dbo.qa_grid_money') IS NOT NULL DROP TABLE dbo.qa_grid_money");
  }
}
