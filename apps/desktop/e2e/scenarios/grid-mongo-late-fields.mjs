// GRID-12 (medium), MongoDB. The grid's columns are the top-level fields of the first batch of
// documents (docs/dev/EXTENDING.md, "one column per top-level field of the first batch"): a field
// that first appears in a later document never gets a column, even after all the documents
// are loaded, so its values cannot be seen, copied or exported from the grid.
// Steps: a collection of 3000 documents {_id, a}, the 2501st also has `late`; open its data,
// scroll to the end so every document is loaded, copy with headers.
// Expected: a `late` column, with 'only here' in that document's row. Actual: only _id and a.
import { assert, connections, copyAll, expand, grid, needs, openData, refresh, sh, status, stubClipboard, treeRow } from "../grid-helpers.mjs";

export const skip = needs("mongo");

export function setup({ dir }) {
  sh("mongo", `db.qa_grid_late.drop();
    const docs = [];
    for (let i = 0; i < 3000; i++) docs.push(i === 2500 ? { _id: i, a: i, late: 'only here' } : { _id: i, a: i });
    db.qa_grid_late.insertMany(docs);`);
  connections(dir, [["QA MONGO", "mongo"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA MONGO", "mongo", "customers");
    await refresh(page, "platypus");
    await page.waitFor(`${treeRow("qa_grid_late")} !== undefined`, 15_000);
    await openData(page, "qa_grid_late");
    // Scrolling to the end loads the rest of the documents.
    for (let i = 0; i < 40 && (await page.eval(status)) !== "3,000 rows" && (await page.eval(status)) !== "3000 rows"; i++) {
      await page.eval(`${grid}.scrollTop = ${grid}.scrollHeight`);
      await page.sleep(250);
    }
    await stubClipboard(page);
    const lines = (await copyAll(page, true)).split("\n");
    assert.equal(lines.length, 3001, `rows: ${await page.eval(status)}`);
    assert.deepEqual(lines[0].split("\t"), ["_id", "a", "late"]);
    assert.equal(lines[2501], "2500\t2500\tonly here");
  } finally {
    sh("mongo", "db.qa_grid_late.drop()");
  }
}
