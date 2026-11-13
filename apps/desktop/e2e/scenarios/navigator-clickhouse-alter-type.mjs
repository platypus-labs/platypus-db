// Bug NAV: on ClickHouse, changing a column's type in the table editor generates PostgreSQL
// syntax, `ALTER TABLE … ALTER COLUMN qty TYPE Int64 USING qty::Int64`, which ClickHouse
// rejects (SYNTAX_ERROR). ClickHouse has no transactions, so the statements before it have
// already run: in the same save a column rename is applied and a new column is not, leaving the
// table half-changed — and, unlike MySQL, the dialog gives no warning that the script is not
// atomic.
//
// Cause: `alter_column` in crates/platypus-schema/src/ddl.rs has no ClickHouse case (it needs
// `ALTER TABLE t MODIFY COLUMN qty Int64`); `script()` warns about partial application for
// MySQL only.
//
// Steps: Edit table… on qa_nav_ch_alter, change qty's type from Nullable(Int32) to
// Nullable(Int64), Save…, Apply.
// Expected: the type changed. Actual: syntax error, type unchanged.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, menu, needs, row, saveConnections } from "../navigator-helpers.mjs";
import { cell, confirm, editor, fill, openSave } from "../table-editor-helpers.mjs";

export const skip = needs("clickhouse");

const DB = "qa_nav_ch_te";

export function setup({ dir }) {
  DBS.clickhouse.exec(`DROP DATABASE IF EXISTS ${DB}; CREATE DATABASE ${DB};
    CREATE TABLE ${DB}.qa_nav_ch_alter (id Int32, qty Nullable(Int32)) ENGINE = MergeTree ORDER BY id;`);
  saveConnections(dir, DBS.clickhouse.connection("QA CH", "00000000-0000-4000-8000-0000000a0062"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA CH", "platypus");
    await page.waitFor(`${row(DB)} !== undefined`, 20_000);
    await expand(page, DB, "qa_nav_ch_alter");
    await menu(page, "qa_nav_ch_alter", "Edit table…");
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 2`, 10_000);
    await fill(page, cell(1, 1), "Nullable(Int64)");
    const sql = await openSave(page);
    const error = await confirm(page);
    assert.equal(error, null, `script:\n${sql}`);
    const type = DBS.clickhouse.exec(`SELECT type FROM system.columns WHERE database = '${DB}' AND table = 'qa_nav_ch_alter' AND name = 'qty'`).trim();
    assert.equal(type, "Nullable(Int64)");
  } finally {
    DBS.clickhouse.exec(`DROP DATABASE IF EXISTS ${DB};`);
  }
}
