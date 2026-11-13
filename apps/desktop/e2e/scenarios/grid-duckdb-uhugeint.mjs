// QA4 grid2 DuckDB (medium: wrong number shown). A UHUGEINT (unsigned 128-bit) above 2^127 is
// shown as a negative number: the maximum 340282366920938463463374607431768211455 reads -1, and
// 2^127 reads -170141183460469231731687303715884105728 (the same bits read as a signed
// HUGEINT). Values up to 2^127 - 1 are right.
// Steps: DuckDB editor, SELECT 340282366920938463463374607431768211455::UHUGEINT,
// 170141183460469231731687303715884105728::UHUGEINT; copy the result.
// Expected: the two numbers as DuckDB prints them. Actual: -1 and -170141183460469231731687303715884105728.
// Code (likely): the DuckDB driver (crates/platypus-driver-duckdb) reads the result through DuckDB's Arrow
// export, which turns UHUGEINT into a signed 128-bit column, so the top half of the range wraps.
import { join } from "node:path";
import { assert, connections, copyAll, openEditor, run, stubClipboard } from "../grid-helpers.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "q.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA DUCK", null, null);
  await stubClipboard(page);
  await run(page, "SELECT 340282366920938463463374607431768211455::UHUGEINT AS a, 170141183460469231731687303715884105728::UHUGEINT AS b");
  assert.equal(await copyAll(page), "340282366920938463463374607431768211455\t170141183460469231731687303715884105728");
}

