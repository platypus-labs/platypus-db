// GRID-01 (high), PostgreSQL. A table whose primary key column needs quoting ("Id", "Order ID",
// a reserved word) opens read-only: "Read-only: the result does not include the table's key
// columns." The key columns come from pg_get_indexdef(), which quotes them ("\"Id\""), and the
// editor looks them up by name among the result's columns
// (crates/platypus-driver-pg/src/meta.rs, indexes(); apps/desktop/src/grid/edit/meta.ts, editMeta).
// Steps: open the data of qa_grid_key."Mixed" (key "Id") and of qa_grid_key.spaced (key "Order ID").
// Expected: both editable, an edit saves. Actual: both read-only.
import { assert, connections, editBar, expand, focusCell, needs, openData, refresh, save, setCell, sh, treeRow } from "../grid-helpers.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_key CASCADE; CREATE SCHEMA qa_grid_key;
    CREATE TABLE qa_grid_key."Mixed" ("Id" int PRIMARY KEY, v text);
    INSERT INTO qa_grid_key."Mixed" VALUES (1, 'a');
    CREATE TABLE qa_grid_key.spaced ("Order ID" int PRIMARY KEY, "select" text);
    INSERT INTO qa_grid_key.spaced VALUES (1, 'a');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_key")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_key")}.querySelector('.twisty').click()`);
    await page.waitFor(`${treeRow("spaced")} !== undefined`, 15_000);
    for (const [table, expected] of [
      ["Mixed", "1\tb"],
      ["spaced", "1\tb"],
    ]) {
      await openData(page, table);
      await page.waitFor(`${editBar} !== '' && !${editBar}.includes('Checking')`, 15_000);
      const bar = await page.eval(editBar);
      assert.ok(!bar.startsWith("Read-only"), `${table}: ${bar}`);
      await focusCell(page, 0, 1);
      await setCell(page, "b");
      assert.match(await save(page), /to edit a cell/);
      const quoted = table === "Mixed" ? '"Mixed"' : "spaced";
      assert.equal(sh("pg", `SELECT * FROM qa_grid_key.${quoted}`), expected);
      await page.key("w", "KeyW", 87, 2);
    }
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_key CASCADE");
  }
}
