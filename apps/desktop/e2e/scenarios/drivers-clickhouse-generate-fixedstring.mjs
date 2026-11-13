// DBS-09 (medium): "Generate data…" adds nothing to a ClickHouse table with a FixedString(n)
// column: the text generator writes values longer than n and ClickHouse rejects the whole
// insert ("String too long for type FixedString(4) … (TOO_LARGE_STRING_SIZE)"). The cut to
// the column's length that varchar(n) and char(n) get does not know FixedString(n), nor
// Nullable(FixedString(n)) / LowCardinality(FixedString(n)).
//
// Steps: ClickHouse (port 58123); qa_dbs.g (id UInt64, code FixedString(4), name String)
// ENGINE = MergeTree ORDER BY id; navigator › g › Generate data…, keep the suggestions, 10 rows.
// Expected: "Added 10 rows." (codes of at most 4 bytes).
// Actual: "Nothing was added: String too long for type FixedString(4) …".
// Suspected: crates/platypus-transfer/src/import.rs:475 `text_length` (only varchar / char /
// nvarchar / nchar / character), used by fake.rs to cut generated text.
import assert from "node:assert/strict";
import { conns, needsPorts, openPath, sh } from "../drivers-helpers.mjs";
import { generate } from "../transfer-helpers.mjs";

export const skip = needsPorts(58123);

export function setup({ dir }) {
  sh("clickhouse", "DROP DATABASE IF EXISTS qa_dbs; CREATE DATABASE qa_dbs; CREATE TABLE qa_dbs.g (id UInt64, code FixedString(4), name String) ENGINE = MergeTree ORDER BY id;");
  conns(dir, [["QA CH", "clickhouse"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA CH", "qa_dbs", "Tables", "g"]);
    const { kinds, out } = await generate(page, "g", ["QA CH", "qa_dbs"], 10);
    assert.match(out, /Added 10 rows/, `${out} (generators ${kinds})`);
    assert.equal(sh("clickhouse", "SELECT count() FROM qa_dbs.g"), "10");
  } finally {
    sh("clickhouse", "DROP DATABASE IF EXISTS qa_dbs");
  }
}
