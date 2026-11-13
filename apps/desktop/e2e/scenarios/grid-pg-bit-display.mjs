// GRID-05 (critical: wrong data shown), PostgreSQL. Values of types the driver has no binary
// decoder for are shown as their wire bytes: bit(n) B'1010' reads "\x00000004a0" and B'0000'
// comes out as control characters; bit varying, macaddr8 ('08:00:2b:01:02:03:04:05' reads
// "\b\0+\1\2\3\4\5"), tsvector, tsquery, pg_lsn ("\x00000016b374d848") and anonymous records
// (ROW(1, 'a')) are garbled the same way; regclass shows the OID (1259) instead of the name.
// The bytes fall through to the UTF-8/bytea fallback of
// crates/platypus-driver-pg/src/decode.rs (format_value, the `_ =>` arm). Copies, exports,
// filters and the undo script of a deleted row all carry the wrong text.
// Expected: the values as psql prints them. Actual: wire bytes.
import { assert, connections, copyAll, expand, needs, openData, refresh, sh, stubClipboard, treeRow } from "../grid-helpers.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_bit CASCADE; CREATE SCHEMA qa_grid_bit;
    CREATE TABLE qa_grid_bit.bits (id int PRIMARY KEY, b bit(4), vb bit varying(8), m8 macaddr8, tv tsvector, lsn pg_lsn);
    INSERT INTO qa_grid_bit.bits VALUES
      (1, B'1010', B'101', '08:00:2b:01:02:03:04:05', 'fox:3 quick:2', '16/B374D848'),
      (2, B'0000', B'', NULL, NULL, NULL);`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_bit")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_bit")}.querySelector('.twisty').click()`);
    await openData(page, "bits");
    await stubClipboard(page);
    assert.equal(
      await copyAll(page),
      "1\t1010\t101\t08:00:2b:01:02:03:04:05\t'fox':3 'quick':2\t16/B374D848\n2\t0000\t\t\t\t",
    );
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_bit CASCADE");
  }
}
