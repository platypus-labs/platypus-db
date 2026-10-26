// GRID-05 (critical: wrong data shown), PostgreSQL. bit(n) and bit varying values are shown as
// their binary wire bytes: B'1010' in a bit(4) column reads "\x00000004a0", and B'0000' comes out
// as control characters. The decoder has no case for BIT/VARBIT, so the bytes (a length and
// the packed bits) fall through to the UTF-8/bytea fallback
// (crates/platypus-driver-pg/src/decode.rs, format_value). Copies, exports, filters and the undo
// script of a deleted row all carry the wrong text.
// Expected: 1010, 0000 and 101 as psql prints them. Actual: \x00000004a0 and garbage.
import { assert, connections, copyAll, expand, needs, openData, refresh, sh, stubClipboard, treeRow } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_bit CASCADE; CREATE SCHEMA qa_grid_bit;
    CREATE TABLE qa_grid_bit.bits (id int PRIMARY KEY, b bit(4), vb bit varying(8));
    INSERT INTO qa_grid_bit.bits VALUES (1, B'1010', B'101'), (2, B'0000', B'');`);
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
    assert.equal(await copyAll(page), "1\t1010\t101\n2\t0000\t");
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_bit CASCADE");
  }
}
