// Regression guard (works): SQL on results with a script's several results (r1, r2, r3 and
// `result` for the one shown), identifiers that need quoting (spaces, quotes, reserved words),
// repeated column names made unique, and a result not loaded in full: the field loads all its
// rows before querying them. Errors stay under the field.
import assert from "node:assert/strict";
import { gridTsv, inField, liteConnection, newEditor, runQuery, runScript, status } from "../qa-ana.mjs";

const NAME = "QA ana sql results";

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0121",
    `CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
     INSERT INTO items VALUES (1, 1, 'a'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, NULL), (5, 1, 'c');
     CREATE TABLE "odd names" ("my col" TEXT, "select" INTEGER, "a""q" TEXT);
     INSERT INTO "odd names" VALUES ('x', 2, 'q'), ('y', 4, 'r');
     CREATE TABLE many (id INTEGER PRIMARY KEY, cat TEXT);
     WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 30000)
       INSERT INTO many SELECT n, 'c' || (n % 120) FROM g;`,
  );
}

export default async function (page) {
  await newEditor(page, NAME);

  await runQuery(page, `SELECT * FROM "odd names"`);
  let run = await inField(page, `"my col" = 'x'`);
  assert.match(run.mark, /^SQL view · 1 row/);
  run = await inField(page, `SELECT "a""q", "select" FROM result ORDER BY 2`);
  assert.equal(await gridTsv(page), `"a""q"\tselect\nq\t2\nr\t4`);

  await runQuery(page, `SELECT 1 AS a, 2 AS a, 'x y' AS "my col", 4 AS "A"`);
  await inField(page, "SELECT * FROM result");
  assert.equal(await gridTsv(page), "a\ta_2\tmy col\tA_3\n1\t2\tx y\t4");

  run = await inField(page, "SELECT nope FROM result");
  assert.match(run.error, /nope/);

  await runScript(page, "SELECT id, name FROM items WHERE id < 3;\nSELECT id FROM items WHERE grp = 1;\nSELECT grp FROM items");
  run = await inField(page, "SELECT (SELECT count(*) FROM r1) AS a, (SELECT count(*) FROM r2) AS b, (SELECT count(*) FROM r3) AS c, (SELECT count(*) FROM result) AS d");
  assert.equal(run.error, "");
  assert.equal(await gridTsv(page), "a\tb\tc\td\n2\t3\t5\t5");

  await runQuery(page, "SELECT * FROM many");
  assert.match(await page.eval(status), /loaded · more available/);
  await inField(page, "SELECT count(*) AS n, count(DISTINCT cat) AS cats FROM result");
  assert.equal(await gridTsv(page), "n\tcats\n30000\t120");
}
