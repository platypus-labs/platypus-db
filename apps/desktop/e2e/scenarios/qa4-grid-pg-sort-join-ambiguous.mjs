// QA4 grid, PostgreSQL (medium: a basic grid gesture fails). Clicking the header of a column in
// the result of a join whose two tables both have an `id` (or `name`) column writes
// `ORDER BY "id" ASC`, which PostgreSQL rejects ("ORDER BY "id" is ambiguous"); the grid is
// replaced by the error and the user has to undo the editor change by hand.
// Steps: open the data of a table, replace the editor text with
//   SELECT a.id, b.id, a.name, b.name FROM a JOIN b ON b.a_id = a.id
// run it and click the header of the first column.
// Expected: the rows come back sorted (the sort uses the column's position or its qualified
// name `a.id`, or the header refuses with the "cannot be sorted from the header" message).
// Actual: error 42702 "ORDER BY "id" is ambiguous" and no rows.
// Code: set_order_by in crates/platypus-sql/src/rewrite.rs quotes only the column's displayed
// name (quote_ident(column)); it should use the ordinal when the result has duplicate names.
import { assert, connections, expand, openData, refresh, treeRow, needs, run, sh, grid, clearStatus, editorText, visibleTab } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP TABLE IF EXISTS public.qa4g_a, public.qa4g_b; CREATE TABLE public.qa4g_a (id int PRIMARY KEY, name text); CREATE TABLE public.qa4g_b (id int PRIMARY KEY, a_id int, name text);
   INSERT INTO public.qa4g_a VALUES (1,'x'),(2,'y'),(3,'z'); INSERT INTO public.qa4g_b VALUES (10,3,'p'),(20,1,'q'),(30,2,'r');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa4g_a")} !== undefined`, 15_000);
    await openData(page, "qa4g_a");
    await run(page, "SELECT a.id, b.id, a.name, b.name FROM public.qa4g_a a JOIN public.qa4g_b b ON b.a_id = a.id");
    await page.eval(`document.querySelectorAll('#qa-grid').forEach(e => e.removeAttribute('id')); ${grid}.id = 'qa-grid'`);
    const r = await page.rect("#qa-grid");
    await clearStatus(page);
    await page.click(r.x + 36, r.y + 20);
    await page.sleep(3000);
    const text = await page.eval(`${visibleTab}.innerText`);
    assert.ok(!/ambiguous/.test(text), "the sort fails: " + text.replace(/\s+/g, " ").slice(-160));
  } finally {
    sh("pg", "DROP TABLE IF EXISTS public.qa4g_a, public.qa4g_b");
  }
}
