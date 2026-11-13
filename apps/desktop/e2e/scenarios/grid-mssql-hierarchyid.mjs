// QA4 grid, SQL Server (high: a whole table cannot be read). A table with a hierarchyid column
// (org charts, folder trees, AdventureWorks' HumanResources.Employee.OrganizationNode) cannot
// be read: any SELECT that includes the column fails and the data viewer shows nothing.
// Steps: CREATE TABLE with id int and node hierarchyid ('/1/2.3/'); run SELECT id, node FROM it.
// Expected: 2 rows; the hierarchyid shown as its bytes (or '/1/2.3/'), like the geography column
// next to it. Actual: the result is an error "Protocol error: invalid token type 0" (and no rows).
// Code: the hierarchyid is a CLR user type (TDS type UDT); geography works but hierarchyid does
// not, so the UDT token decoding in vendor/tiberius (column_data / type_info) or the metadata
// of crates/platypus-driver-mssql/src/columns.rs mis-reads this one.
import { assert, connections, needs, openEditor, run, rows, sh, status, stubClipboard } from "../grid-helpers.mjs";

export const skip = needs("mssql");

export function setup({ dir }) {
  sh("mssql", `IF OBJECT_ID('dbo.qa4g_h') IS NOT NULL DROP TABLE dbo.qa4g_h;
CREATE TABLE dbo.qa4g_h (id int PRIMARY KEY, node hierarchyid, name nvarchar(20));
INSERT INTO dbo.qa4g_h VALUES (1, '/1/2.3/', N'a'), (2, NULL, N'b');
GO
`);
  connections(dir, [["QA MS", "mssql"]]);
}

export default async function (page) {
  try {
    await openEditor(page, "QA MS", "mssql", "platypus");
    await stubClipboard(page);
    await run(page, "SELECT id, node, name FROM dbo.qa4g_h ORDER BY id").catch(() => {});
    await page.sleep(1000);
    const text = await page.eval(`[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none').innerText`);
    assert.ok(!/Protocol error/.test(text), "the query fails: " + text.replace(/\s+/g, " ").slice(-120));
    const r = await rows(page);
    assert.equal(r.length, 2);
    assert.equal(r[0][2], "a");
  } finally {
    try { sh("mssql", "DROP TABLE dbo.qa4g_h"); } catch {}
  }
}
