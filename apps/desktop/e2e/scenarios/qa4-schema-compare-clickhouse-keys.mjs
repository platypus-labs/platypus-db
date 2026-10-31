// SCH4-08 (medium). Compare with... on ClickHouse creates the table without its ORDER BY, PARTITION BY and
// TTL (the server falls back to its default engine), and when the primary key is shorter than the
// sorting key it lists the sorting key as an index named "ORDER BY" and runs CREATE INDEX `ORDER BY`,
// which fails. The target is a different table (other sort order, no partitions, no TTL), silently.
// Steps: qa4ch_src.t (id UInt64, ts DateTime) ENGINE = MergeTree PARTITION BY toYYYYMM(ts)
// PRIMARY KEY id ORDER BY (id, ts) TTL ts + INTERVAL 30 DAY, and an empty qa4ch_dst; navigator >
// qa4ch_src > Compare with... > QA4 CH - qa4ch_dst > run the script.
// Expected: the script runs and system.tables of the target has the source's sorting_key, partition_key
// and TTL (or the dialog refuses to compare ClickHouse tables). Actual: the script
// "CREATE TABLE qa4ch_dst.t (id UInt64, ts DateTime, PRIMARY KEY (id)); CREATE INDEX `ORDER BY` ON
// qa4ch_dst.t (id, ts);" fails (or, with PRIMARY KEY (id, ts), the table has no PARTITION BY / TTL).
// Code: crates/platypus-schema/src/lib.rs load() (options: None for ClickHouse; the sorting key shows up
// as an IndexDesign named "ORDER BY") and ddl.rs Ddl::create_table / create_index.
import assert from "node:assert/strict";
import { DBS, needs, saveConnections } from "../qa-nav-helpers.mjs";
import { compareUi, openPath, runEditorScript } from "../qa3-schema.mjs";

export const skip = needs("clickhouse");
const DB = "qa4ch_src";
const DST = "qa4ch_dst";
const drop = () => DBS.clickhouse.exec(`DROP DATABASE IF EXISTS ${DB}; DROP DATABASE IF EXISTS ${DST}`);

export function setup({ dir }) {
  drop();
  DBS.clickhouse.exec(`CREATE DATABASE ${DB}; CREATE DATABASE ${DST};
CREATE TABLE ${DB}.t (id UInt64, ts DateTime) ENGINE = MergeTree PARTITION BY toYYYYMM(ts) PRIMARY KEY id ORDER BY (id, ts) TTL ts + INTERVAL 30 DAY;`);
  saveConnections(dir, DBS.clickhouse.connection("QA4 CH", "00000000-0000-4000-8000-0000000c4501", 'password_command = "echo platypus"'));
}

export default async function (page) {
  try {
    await openPath(page, ["QA4 CH", DB]);
    const first = await compareUi(page, ["QA4 CH"], DB, "QA4 CH", DST);
    assert.ok(first.sql, JSON.stringify(first));
    const error = await runEditorScript(page);
    assert.equal(error, null, `the script fails: ${error}\n${first.sql}`);
    const keys = (db) => DBS.clickhouse.exec(`SELECT sorting_key, partition_key, ifNull(extract(create_table_query, 'TTL [^\\n]*'), '') FROM system.tables WHERE database = '${db}' AND name = 't' FORMAT TSV`).trim();
    assert.equal(keys(DST).replace(/qa4ch_dst/g, "x"), keys(DB).replace(/qa4ch_src/g, "x"), first.sql);
  } finally {
    drop();
  }
}
