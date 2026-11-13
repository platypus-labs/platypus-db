// QA4 grid, PostgreSQL (medium). The quick filter "Filter > Equal to this value" (and "Not equal",
// "In selected values") on a `json` or `xml` cell writes `col = '<literal>'`, but PostgreSQL has
// no equality operator for json, xml (nor for point-like types without btree): the viewer shows
// the raw error "42883: operator does not exist: json = unknown" and the grid is replaced by it.
// Steps: table with a json column (and an xml column); open its data; focus the json cell of
// row 1; grid menu > Filter > Equal to this value.
// Expected: the filter works (e.g. `col::text = '...'`) or the entry is disabled with a reason,
// as it is for computed columns. Actual: the statement fails with 42883 and no rows are shown.
// Code: apps/desktop/src/grid/features/filters.ts (byValue writes `${name} = ${literal}`
// without looking at the column type).
import { assert, connections, expand, openData, refresh, treeRow, needs, sh, grid, focusCell, gridMenu, visibleTab, clearStatus } from "../grid-helpers.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP TABLE IF EXISTS public.qa4g_j; CREATE TABLE public.qa4g_j (id int PRIMARY KEY, doc json, x xml);
    INSERT INTO public.qa4g_j VALUES (1, '{"a": 1}', '<a>1</a>'), (2, '{"a": 2}', '<a>2</a>');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa4g_j")} !== undefined`, 15_000);
    await openData(page, "qa4g_j");
    await page.eval(`document.querySelectorAll('#qa-grid').forEach(e => e.removeAttribute('id')); ${grid}.id = 'qa-grid'`);
    await focusCell(page, 0, 1);
    await clearStatus(page);
    await gridMenu(page, "Filter", "Equal to this value");
    await page.sleep(3000);
    const text = await page.eval(`${visibleTab}.innerText`);
    assert.ok(!/operator does not exist/.test(text), "the filter fails: " + text.replace(/\s+/g, " ").slice(-200));
  } finally {
    sh("pg", "DROP TABLE IF EXISTS public.qa4g_j");
  }
}
