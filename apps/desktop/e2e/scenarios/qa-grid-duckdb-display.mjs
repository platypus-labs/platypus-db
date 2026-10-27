// GRID-11 (medium: wrong data shown), DuckDB. Values of nested and bit types are shown in
// Arrow's debug format rather than as DuckDB writes them:
// - BIT '1010' reads "04fa" (the bytes of DuckDB's bit storage);
// - a NULL element of a list reads as nothing: [1, NULL, 3] -> "[1, , 3]", the same as an
//   empty string element ['a', '', 'b'] -> "[a, , b]";
// - strings inside lists, structs and maps lose their quotes: {'a': 1, 'b': 'x'} -> "{a: 1, b: x}".
// The text is not a DuckDB literal either, so it cannot be edited back.
// (apps/desktop/src-tauri/src/display.rs renders every column with arrow_cast's ArrayFormatter.)
// Expected: 1010, [1, NULL, 3], ['a', '', 'b'], {'a': 1, 'b': 'x'} (DuckDB literals).
import { join } from "node:path";
import { assert, connections, copyAll, run, stubClipboard, treeRow, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "qa.duckdb") }]]);
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA DUCK")} !== undefined`);
  await page.eval(`${treeRow("QA DUCK")}.id = 'qa-conn'`);
  await page.clickSel("#qa-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await stubClipboard(page);
  assert.equal(
    await run(page, "SELECT '1010'::BIT AS b, [1, NULL, 3] AS l, ['a', '', 'b'] AS s, {'a': 1, 'b': 'x'} AS st"),
    "1 row",
  );
  assert.equal(await copyAll(page), "1010\t[1, NULL, 3]\t['a', '', 'b']\t{'a': 1, 'b': 'x'}");
}
