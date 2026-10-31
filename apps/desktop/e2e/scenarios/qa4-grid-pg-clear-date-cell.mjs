// QA4 grid, PostgreSQL (medium: confusing error from a basic edit). Clearing a date, time or
// timestamp cell (empty the date input, Enter) makes the pending change `d = ''`, and saving
// fails with `invalid input syntax for type date: ""`. Clearing an integer or numeric cell
// gives NULL, as the user expects, and a boolean cell says "'' is not true or false".
// Steps: table with a nullable date column holding a date; open a SELECT on it; F2 on the date
// cell, empty the input, Enter; Preview SQL; Save.
// Expected: the cell becomes NULL (like the integer column does) or the editor refuses with
// a clear client-side message. Actual: `UPDATE ... SET d = ''` and the PostgreSQL error.
// Code: the date/time cell editor (apps/desktop/src/grid, the cell editor commit) sends the
// empty string as the typed text; the "empty means NULL" rule applies to the number columns only.
import { assert, connections, needs, openEditor, previewSql, run, setCell, sh, focusCell, button, visibleTab } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP TABLE IF EXISTS public.qa4g_d; CREATE TABLE public.qa4g_d (id int PRIMARY KEY, n int, d date, ts timestamp, tm time);
    INSERT INTO public.qa4g_d VALUES (1, 5, '2024-01-01', '2024-01-01 10:00', '10:00');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await openEditor(page, "QA PG", "pg", "public");
    await run(page, "SELECT * FROM public.qa4g_d ORDER BY id");
    await focusCell(page, 0, 1);
    await setCell(page, "");
    assert.match(await previewSql(page), /SET n = NULL/, "an emptied number cell is NULL");
    await page.eval(`${button(visibleTab, "Discard")}.click()`);
    await page.sleep(300);
    await focusCell(page, 0, 2);
    await setCell(page, "");
    const sql = await previewSql(page);
    assert.ok(!/d = ''/.test(sql), "the emptied date cell is written as an empty string: " + sql);
  } finally {
    sh("pg", "DROP TABLE IF EXISTS public.qa4g_d");
  }
}
