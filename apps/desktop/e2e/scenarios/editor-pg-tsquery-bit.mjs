// SQL-19. PostgreSQL tsquery and bit/varbit values are shown as their raw binary wire format:
// 'a & b'::tsquery shows NUL bytes and letters ("\u0000\u0000\u0000\u0003\u0002..."),
// B'101' shows "\x00000003a0". tsvector, likewise used in full-text search, is worth a look.
// Steps: PostgreSQL, Ctrl+Enter on the SELECT below, copy the grid with headers.
// Expected: 'a' & 'b' and 101.
// Actual: binary garbage and a hex dump.
// Code: crates/platypus-driver-pg type mapping: unknown types fall back to their binary bytes.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "SELECT 'a & b'::tsquery AS q, B'101' AS bits");
  assert.equal(await gridTsv(page), "q\tbits\n'a' & 'b'\t101");
}
