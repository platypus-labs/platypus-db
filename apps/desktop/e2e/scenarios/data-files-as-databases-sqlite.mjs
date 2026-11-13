// Data files as databases (DuckDB reading the file at each query): a JSON array (view named
// after the file, nested lists usable), newline-delimited JSON, an Excel workbook (one table
// per sheet, queried together), a Parquet file dropped on the window (written by a COPY from
// the JSON one) that becomes a saved connection; a JSON file that is not JSON fails to open
// with the reason. Needs the DuckDB driver the e2e runner installs.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { openQueryTab, run, treeRow } from "../conn-helpers.mjs";
import { rows, stubClipboard } from "../grid-helpers.mjs";
import { writeXlsx } from "../xlsx.mjs";

const file = (name) => name.replaceAll("\\", "/");
const duck = (id, name, path) => `
[[connections]]
id = "00000000-0000-4000-8000-0000000000${id}"
name = "${name}"
driver = "duckdb"
database = '${path}'
save_password = false
`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "data.json"),
    JSON.stringify([
      { id: 1, name: "ann", tags: ["a", "b"] },
      { id: 2, name: "bob", tags: [] },
      { id: 3, name: "cy", tags: ["c"] },
    ]),
  );
  writeFileSync(join(dir, "events.ndjson"), '{"kind":"open","n":1}\n{"kind":"close","n":2}\n{"kind":"open","n":3}\n');
  writeFileSync(join(dir, "broken.json"), "this is {not json");
  writeXlsx(join(dir, "books.xlsx"), {
    Fruit: [
      ["name", "qty"],
      ["apple", 3],
      ["pear", 5],
    ],
    Veg: [
      ["name", "qty"],
      ["kale", 7],
    ],
  });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${duck("d1", "data.json", join(dir, "data.json"))}${duck("d2", "events.ndjson", join(dir, "events.ndjson"))}${duck("d3", "books.xlsx", join(dir, "books.xlsx"))}${duck("d4", "broken.json", join(dir, "broken.json"))}`,
  );
}

/** Expands the connection `name` and opens a script on it. */
async function open(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await openQueryTab(page, name);
}

export default async function (page, { dir }) {
  await stubClipboard(page);
  // A JSON array: the view is named after the file; a nested list is a list.
  await open(page, "data.json");
  const first = await run(page, "SELECT id, name, len(tags) AS n FROM data ORDER BY id");
  assert.equal(first, "3 rows");
  assert.deepEqual(await rows(page), [["1", "ann", "2"], ["2", "bob", "0"], ["3", "cy", "1"]]);
  await run(page, "SELECT name FROM data WHERE list_contains(tags, 'c')");
  assert.deepEqual(await rows(page), [["cy"]]);

  // The file as Parquet, written from that view, and dropped on the window like a file from the
  // desktop: a connection dialog on it, saved under the file's name.
  const parquet = file(join(dir, "data.parquet"));
  await run(page, `COPY (SELECT id, name FROM data) TO '${parquet}' (FORMAT parquet)`);
  await page.eval(`window.__platypus.openDroppedFile(${JSON.stringify(join(dir, "data.parquet"))})`);
  await page.waitFor(`document.querySelector('.modal input')?.value === 'data.parquet'`, 15_000);
  await page.eval(`[...document.querySelector('.modal').querySelectorAll('button')].find(b => b.textContent.trim() === 'Save').click()`);
  await page.waitFor(`${treeRow("data.parquet")} !== undefined`, 15_000);
  await openQueryTab(page, "data.parquet");
  await run(page, "SELECT name FROM data ORDER BY id DESC");
  assert.deepEqual(await rows(page), [["cy"], ["bob"], ["ann"]]);

  // Newline-delimited JSON, one object a line.
  await open(page, "events.ndjson");
  await run(page, "SELECT kind, count(*) AS n FROM events GROUP BY kind ORDER BY kind");
  assert.deepEqual(await rows(page), [["close", "1"], ["open", "2"]]);

  // A workbook: a table a sheet, joined in one query.
  await open(page, "books.xlsx");
  await run(page, `SELECT name FROM "Fruit" UNION ALL SELECT name FROM "Veg" ORDER BY name`);
  assert.deepEqual(await rows(page), [["apple"], ["kale"], ["pear"]]);
  await run(page, `SELECT sum(qty) AS total FROM (SELECT qty FROM "Fruit" UNION ALL SELECT qty FROM "Veg")`);
  assert.deepEqual(await rows(page), [["15"]]);

  // A file that is not what its extension says: the connection fails and says why.
  await page.eval(`${treeRow("broken.json")}.querySelector('.twisty').click()`);
  await page.waitFor(`/cannot read/i.test(document.body.textContent)`, 20_000);
  assert.equal(await page.eval(`document.querySelectorAll('.query-tab').length`), 4, "no tab opened for it");
}
