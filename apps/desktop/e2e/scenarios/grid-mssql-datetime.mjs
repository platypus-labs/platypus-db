// GRID-07 (high), SQL Server. Date-time values are read as microsecond timestamps
// (crates/platypus-driver-mssql/src/columns.rs, ColumnBuilder::Timestamp for Datetime,
// Datetime2, DateTimeOffset), which:
// - shows `datetime` with its internal 1/300 s ticks: '2024-01-02 03:04:05.997' reads
//   '2024-01-02 03:04:05.996667' (SSMS and DBeaver show .997);
// - drops the 7th digit of datetime2(7) and datetimeoffset(7): '...05.1234567' reads '...05.123456';
// - shows datetimeoffset in UTC, not at its own offset.
// The editor identifies rows by that text, so a table keyed by a `datetime` cannot save an
// edit ("Conversion failed when converting date and/or time from character string": datetime
// takes 3 decimals), and one keyed by datetime2(7) cannot either ("the row was not found").
// Expected: the stored values, and edits that save. Actual: altered text, failed saves.
import { openEditor, assert, button, connections, copyAll, editBar, focusCell, needs, run, save, setCell, sh, stubClipboard, treeRow, visibleTab } from "../grid-helpers.mjs";

export const skip = needs("mssql");

export function setup({ dir }) {
  sh("mssql", `IF OBJECT_ID('dbo.qa_grid_dt') IS NOT NULL DROP TABLE dbo.qa_grid_dt;
    IF OBJECT_ID('dbo.qa_grid_dt2') IS NOT NULL DROP TABLE dbo.qa_grid_dt2;
    CREATE TABLE dbo.qa_grid_dt (k datetime PRIMARY KEY, v nvarchar(10));
    INSERT INTO dbo.qa_grid_dt VALUES ('2024-01-02 03:04:05.997', N'a');
    CREATE TABLE dbo.qa_grid_dt2 (k datetime2(7) PRIMARY KEY, o datetimeoffset(7), v nvarchar(10));
    INSERT INTO dbo.qa_grid_dt2 VALUES ('2024-01-02 03:04:05.1234567', '2024-06-01 12:00:00.1234567 +02:00', N'a');`);
  connections(dir, [["QA MS", "mssql"]]);
}

/** Runs `sql`, edits the `v` column of its first row and saves; returns the save's outcome. */
async function editFirst(page, sql, col) {
  assert.equal(await run(page, sql), "1 row");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 30_000);
  await focusCell(page, 0, col);
  await setCell(page, "b");
  const outcome = await save(page);
  await page.eval(`${button(visibleTab, "Discard")}?.click()`);
  return outcome;
}

export default async function (page) {
  try {
    await openEditor(page, "QA MS", "mssql", "platypus");
    await stubClipboard(page);

    assert.equal(await run(page, "SELECT k FROM dbo.qa_grid_dt"), "1 row");
    const shown = await copyAll(page);
    assert.equal(await run(page, "SELECT k, o FROM dbo.qa_grid_dt2"), "1 row");
    const shown2 = await copyAll(page);

    const dt = await editFirst(page, "SELECT k, v FROM dbo.qa_grid_dt", 1);
    const dt2 = await editFirst(page, "SELECT k, v FROM dbo.qa_grid_dt2", 1);
    assert.deepEqual(
      {
        shown,
        shown2,
        saved: sh("mssql", "SET NOCOUNT ON; SELECT (SELECT v FROM dbo.qa_grid_dt) + (SELECT v FROM dbo.qa_grid_dt2)"),
      },
      {
        shown: "2024-01-02 03:04:05.997",
        shown2: "2024-01-02 03:04:05.1234567\t2024-06-01 12:00:00.1234567 +02:00",
        saved: "bb",
      },
      `saves: datetime ${dt} / datetime2 ${dt2}`,
    );
  } finally {
    sh("mssql", "IF OBJECT_ID('dbo.qa_grid_dt') IS NOT NULL DROP TABLE dbo.qa_grid_dt; IF OBJECT_ID('dbo.qa_grid_dt2') IS NOT NULL DROP TABLE dbo.qa_grid_dt2;");
  }
}
