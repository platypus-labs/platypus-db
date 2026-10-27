// Regression guard (passes): on CockroachDB the data editor saves edits of a row keyed by a
// unique_rowid() value above 2^53 (the WHERE keeps every digit) and writes back DECIMAL, JSONB,
// INET, TIMESTAMPTZ, INTERVAL, BYTES, DATE, TIME and INT8 values as typed.
import assert from "node:assert/strict";
import { conns, editBar, focusCell, needsPorts, openData, openPath, previewSql, save, setCell, sh } from "../qa-dbs.mjs";

export const skip = needsPorts(56257);

export function setup({ dir }) {
  sh("cockroach", `DROP SCHEMA IF EXISTS qa_dbs CASCADE; CREATE SCHEMA qa_dbs;
CREATE TABLE qa_dbs.e (id INT8 PRIMARY KEY DEFAULT unique_rowid(), i8 INT8, d DECIMAL(30,10), s STRING, j JSONB, ip INET, ts TIMESTAMPTZ, iv INTERVAL, b BYTES, dt DATE, tm TIME);
INSERT INTO qa_dbs.e (s) VALUES ('first'), ('second');`);
  conns(dir, [["QA CRDB", "cockroach"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA CRDB", "platypus", "qa_dbs", "Tables", "e"]);
    await openData(page, "e");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    const ids = sh("cockroach", "SELECT id::STRING FROM qa_dbs.e ORDER BY id;").split("\n").slice(1);
    assert.ok(BigInt(ids[0]) > 2n ** 53n);
    const values = [[1, "9007199254740995"], [2, "12345678901234567890.0123456789"], [3, "edited ✓ 'q'"], [4, '{"b": [1, null]}'], [5, "10.0.0.1"],
      [6, "2025-06-01 12:00:00+02:00"], [7, "3 days"], [8, "\\xbeef"], [9, "2024-02-29"], [10, "01:02:03"]];
    for (const [col, text] of values) {
      await focusCell(page, 0, col);
      await setCell(page, text);
    }
    assert.match(await previewSql(page), new RegExp(`WHERE id = ${ids[0]};$`));
    assert.match(await save(page), /to edit a cell/);
    const rows = sh("cockroach", "SELECT i8, d, s, j, ip, ts, iv, b, dt, tm FROM qa_dbs.e ORDER BY id;").split("\n");
    assert.equal(rows[1], [
      "9007199254740995", "12345678901234567890.0123456789", "edited ✓ 'q'", '"{""b"": [1, null]}"', "10.0.0.1",
      "2025-06-01 10:00:00+00", "3 days", "\\xbeef", "2024-02-29", "01:02:03",
    ].join("\t"));
    assert.match(rows[2], /\tsecond\t/);
  } finally {
    sh("cockroach", "DROP SCHEMA IF EXISTS qa_dbs CASCADE;");
  }
}
