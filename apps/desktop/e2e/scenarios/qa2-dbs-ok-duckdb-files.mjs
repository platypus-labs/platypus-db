// Regression guard (passes): a DuckDB connection queries files directly: a table copied to
// Parquet and read back keeps its STRUCT and MAP values, and a CSV with a quoted comma, an
// empty field and UTF-8 text is read as typed rows; a view over read_csv shows in the
// navigator.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { conns, openEditor, openPath, rows, run, stubClipboard, treeMenu } from "../qa-dbs.mjs";

export function setup({ dir }) {
  writeFileSync(join(dir, "in.csv"), 'id,name,amount\n1,héllo 😀,1.5\n2,"a,b",\n');
  conns(dir, [["QA Duck", { duckdb: join(dir, "f.duckdb") }]]);
}

export default async function (page, { dir }) {
  const pq = join(dir, "out.parquet").replaceAll("\\", "/");
  const csv = join(dir, "in.csv").replaceAll("\\", "/");
  await openEditor(page, "QA Duck", null);
  await stubClipboard(page);
  await run(page, `CREATE TABLE t (id INTEGER, s STRUCT(a INT, b VARCHAR), m MAP(VARCHAR, INT)); INSERT INTO t VALUES (1, {'a': 1, 'b': 'q'}, MAP {'k': 1});
COPY t TO '${pq}' (FORMAT parquet);
CREATE VIEW vcsv AS SELECT * FROM read_csv('${csv}');`, true);
  await run(page, `SELECT id, s, m FROM '${pq}'`);
  assert.deepEqual(await rows(page), [["1", "{'a': 1, 'b': 'q'}", "{k: 1}"]]);
  await run(page, `SELECT * FROM '${csv}' ORDER BY id`);
  assert.deepEqual(await rows(page), [["1", "héllo 😀", "1.5"], ["2", "a,b", ""]]);
  await treeMenu(page, "QA Duck", "Refresh");
  await openPath(page, ["QA Duck", "main", "Views", "vcsv"]);
}
