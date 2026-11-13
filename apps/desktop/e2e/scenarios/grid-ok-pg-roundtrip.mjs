// Regression guard (passes): the data editor writes PostgreSQL values of many types back as
// typed, through each cell editor (inline text, select for bool and enum, date input, the JSON
// popup) and the array list editor; a conflicting change made elsewhere is reported and the
// pending edit is kept.
import { assert, button, connections, editBar, expand, focusCell, needs, openData, previewSql, refresh, run, save, setCell, sh, treeRow, visibleTab } from "../grid-helpers.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_ok CASCADE; CREATE SCHEMA qa_grid_ok;
    CREATE TYPE qa_grid_ok.mood AS ENUM ('sad', 'ok', 'happy');
    CREATE TABLE qa_grid_ok.types (id int PRIMARY KEY, i8 bigint, n numeric(40,20), d double precision, b boolean, t text, dt date,
      tstz timestamptz, iv interval, u uuid, jb jsonb, tags text[], by bytea, mood qa_grid_ok.mood);
    INSERT INTO qa_grid_ok.types (id) VALUES (1), (2);`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_ok")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_ok")}.querySelector('.twisty').click()`);
    await openData(page, "types");
    await run(page, "SELECT * FROM qa_grid_ok.types ORDER BY id");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    const values = [
      [1, "9223372036854775807"],
      [2, "12345678901234567890.12345678901234567890"],
      [3, "-Infinity"],
      [4, "true"],
      [5, "tab\there 'q' \\ é😀"],
      [6, "2025-12-31"],
      [7, "2025-01-01 10:00:00+05:30"],
      [8, "2 days 03:00:00"],
      [9, "b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12"],
      [10, '{"z": [1, null]}'],
      [12, "\\xCAFE"],
      [13, "happy"],
    ];
    for (const [col, text] of values) {
      await focusCell(page, 0, col);
      await setCell(page, text);
    }
    // The array through its list editor: one element added with a comma and a quote.
    await focusCell(page, 0, 11);
    await page.key("F2", "F2", 113);
    await page.waitFor(`document.querySelector('.list-editor-add') !== null`);
    await page.eval(`document.querySelector('.list-editor-add').click()`);
    await page.waitFor(`document.querySelector('.list-editor-row input:not([type=checkbox])') !== null`);
    await page.type('a,"b"');
    await page.eval(`[...document.querySelectorAll('.list-editor button')].find(b => b.textContent === 'Apply').click()`);
    await page.waitFor(`document.querySelector('.list-editor') === null`);
    assert.match(await previewSql(page), /^UPDATE qa_grid_ok\.types SET .* WHERE id = 1;$/);
    assert.match(await save(page), /to edit a cell/);
    assert.equal(
      sh("pg", "SELECT i8, n, d, b, t, dt, tstz, iv, u, jb, tags, by, mood FROM qa_grid_ok.types WHERE id = 1"),
      [
        "9223372036854775807", "12345678901234567890.12345678901234567890", "-Infinity", "t", "tab\there 'q' \\ é😀", "2025-12-31",
        "2025-01-01 04:30:00+00", "2 days 03:00:00", "b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12", '{"z": [1, null]}', '{"a,\\"b\\""}', "\\xcafe", "happy",
      ].join("\t"),
    );

    // A row changed elsewhere since it was loaded: the save says so and keeps the edit.
    await run(page, "SELECT * FROM qa_grid_ok.types ORDER BY id");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    sh("pg", "UPDATE qa_grid_ok.types SET id = 20 WHERE id = 2");
    await focusCell(page, 1, 5);
    await setCell(page, "conflict");
    assert.match(await save(page), /the row was not found/);
    assert.match(await page.eval(editBar), /1 row changed/);
    await page.eval(`${button(visibleTab, "Discard")}.click()`);
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_ok CASCADE");
  }
}
