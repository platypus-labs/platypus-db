// DBS-21 (low, UX): a DuckDB database attached in the editor (ATTACH 'other.duckdb' AS other)
// never shows in the navigator, even after Refresh: only the opened file's `main` is listed,
// so its tables cannot be browsed, opened, edited, exported or used by the data tools.
// Documented as a limitation (crates/platypus-driver-duckdb/src/meta.rs:2); DBeaver lists
// attached databases as catalogs.
//
// Steps: DuckDB file; editor: ATTACH '<dir>/other.duckdb' AS other; CREATE TABLE other.o (id
// INT); navigator › connection › Refresh.
// Expected: "other" listed next to "main", with its table o.
// Actual: only "main".
// Suspected: crates/platypus-driver-duckdb/src/meta.rs (schemas of the default catalog only).
import { join } from "node:path";
import { conns, openEditor, openPath, run, treeMenu } from "../qa-dbs.mjs";

export function setup({ dir }) {
  conns(dir, [["QA Duck", { duckdb: join(dir, "main.duckdb") }]]);
}

export default async function (page, { dir }) {
  const other = join(dir, "other.duckdb").replaceAll("\\", "/");
  await openEditor(page, "QA Duck", null);
  await run(page, `ATTACH '${other}' AS other; CREATE TABLE other.o (id INT);`, true);
  await treeMenu(page, "QA Duck", "Refresh");
  await openPath(page, ["QA Duck", "other", "Tables", "o"], 15_000);
}
