// SQL-15. The editor's context menu offers "Explain plan" and "Explain analyze" on every SQL
// connection, but on DuckDB, ClickHouse, SQL Server and CockroachDB the explain tab only shows
// the raw text "unsupported: explain" ("unsupported: CockroachDB query plans"), although all
// of them have EXPLAIN.
// Steps: DuckDB, `SELECT * FROM range(10) WHERE range > 1`, context menu > Explain plan.
// Expected: a plan (or no Explain entry where there is none).
// Actual: "unsupported: explain".
// Code: the drivers' Connection::explain (platypus-driver-duckdb, -clickhouse, -mssql) are not
// implemented; apps/desktop/src/components/QueryTab.tsx host.explain is set for every SQL driver.
import assert from "node:assert/strict";
import { editorMenu, openEditor, saveConnections, setEditor } from "../editor-helpers.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { duckdb: true });

const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;

export default async function (page) {
  await openEditor(page, "QA DuckDB");
  await setEditor(page, "SELECT * FROM range(10) WHERE range > 1");
  await editorMenu(page, "Explain plan");
  await page.waitFor(`${explainTab} !== undefined && (${explainTab}.querySelector('.explain-node') !== null || ${explainTab}.textContent.includes('unsupported'))`, 20_000);
  assert.doesNotMatch(await page.eval(`${explainTab}.textContent`), /unsupported/);
}
