// GRID-19 (critical: wrong data shown), PostgreSQL. 'infinity' and '-infinity' dates and
// timestamps (common as an open end: valid_to = 'infinity') are shown as an error message in
// the cell: "ERROR: Cast error: Failed to convert 9223372036854775807 to datetime for
// Timestamp(µs)". The driver stores them as the extreme Arrow values, which arrow_cast cannot
// format (apps/desktop/src-tauri/src/display.rs, render / columns_text with ArrayFormatter).
// The same text is copied, exported and written into undo scripts. Dates far outside the
// Arrow range fail the same way; BC dates read "-2023-01-01" for '2024-01-01 BC', which
// PostgreSQL does not take back.
// Expected: infinity, -infinity and 2024-01-01 BC as psql prints them. Actual: error text and
// an astronomical year.
import { assert, connections, copyAll, expand, needs, openData, refresh, sh, stubClipboard, treeRow } from "../grid-helpers.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_inf CASCADE; CREATE SCHEMA qa_grid_inf;
    CREATE TABLE qa_grid_inf.periods (id int PRIMARY KEY, valid_from date, valid_to timestamp, at timestamptz);
    INSERT INTO qa_grid_inf.periods VALUES (1, '-infinity', 'infinity', 'infinity'), (2, '2024-01-01 BC', '2024-02-01 10:00', '2024-02-01 10:00+00');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_inf")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_inf")}.querySelector('.twisty').click()`);
    await openData(page, "periods");
    await stubClipboard(page);
    assert.equal(
      await copyAll(page),
      "1\t-infinity\tinfinity\tinfinity\n2\t2024-01-01 BC\t2024-02-01 10:00:00\t2024-02-01 10:00:00+00:00",
    );
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_inf CASCADE");
  }
}
