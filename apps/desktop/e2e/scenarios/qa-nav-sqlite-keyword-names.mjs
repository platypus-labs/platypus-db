// Bug NAV: on SQLite, tables named with SQLite keywords that cannot be bare identifiers and
// are not in the shared reserved list — `transaction`, `commit`, `add`, `alter`, `escape`,
// `nothing`, `autoincrement` — are written unquoted: Open data runs
// `SELECT * FROM transaction` and fails with `near "transaction": syntax error`; Drop… and
// the generated statements are broken the same way.
//
// Cause: no SQLite keyword list in `quoteIdent` (apps/desktop/src/sqlNames.ts) nor in
// `Dialect::quote_ident` (crates/platypus-sql/src/dialect.rs).
//
// Steps: right-click the table `transaction` › Open data.
// Expected: its row. Actual: syntax error.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { editorText, menu, row, saveConnections, visibleTab, waitRun } from "../qa-nav-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "kw.db"));
  db.exec(`CREATE TABLE "transaction" (id INTEGER); INSERT INTO "transaction" VALUES (1);
           CREATE TABLE "commit" (id INTEGER);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0041"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "kw.db")}'
save_password = false
`);
}

export default async function (page) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("transaction")} !== undefined`, 15_000);
  await menu(page, "transaction", "Open data");
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`, 10_000);
  const status = await waitRun(page);
  assert.equal(status, "1 row", `Open data ran: ${await page.eval(editorText)}`);

  await menu(page, "commit", "Drop…");
  await page.waitFor(`/DROP TABLE/.test(document.querySelector('.modal.sql-action .sql-preview')?.textContent ?? '')`, 10_000);
  const drop = await page.eval(`document.querySelector('.modal.sql-action .sql-preview').textContent`);
  assert.match(drop, /DROP TABLE (main\.)?"commit";/);
}
