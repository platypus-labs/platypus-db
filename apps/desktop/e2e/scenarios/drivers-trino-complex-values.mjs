// DBS-06 (medium, wrong display): Trino MAP and ROW values are shown, copied and exported in
// Go's debug syntax, and UUIDs lose their dashes: MAP(ARRAY['k'], ARRAY[1]) shows
// "map[k:1]", ROW(1, 'x') AS row(a integer, b varchar) shows "{[a b] [1 x] true}", and
// UUID 'b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12' shows "b0eebc999c0b4ef8bb6d6bb9bd380a12".
// Arrays are fine (['a', NULL]). DBeaver shows {k=1}, {a=1, b=x} and the dashed UUID.
//
// Steps: Trino (docker profile trino); SELECT MAP(ARRAY['k'], ARRAY[1]) AS m,
// CAST(ROW(1, 'x') AS row(a integer, b varchar)) AS r, UUID '…' AS u; copy the grid.
// Expected: the map and the row as structured values ({k: 1}, {a: 1, b: x} — as the app shows
// ClickHouse and DuckDB maps and structs) and the UUID in its 8-4-4-4-12 form.
// Actual: map[k:1], {[a b] [1 x] true}, b0eebc999c0b4ef8bb6d6bb9bd380a12.
// Suspected: the Trino ADBC driver hands these types over as strings formatted by Go (map,
// row) and as 16 raw bytes (uuid); crates/platypus-driver-adbc does not turn them back into
// Arrow maps/structs or a UUID text (see its column type mapping in meta.rs / ipc.rs).
import assert from "node:assert/strict";
import { conns, needsPorts, openEditor, rows, run, seedTrinoDriver, stubClipboard } from "../drivers-helpers.mjs";

export const skip = needsPorts(58085);

export function setup({ dir }) {
  seedTrinoDriver(dir);
  conns(dir, [["QA Trino", "trino"]]);
}

export default async function (page) {
  await openEditor(page, "QA Trino", null);
  await stubClipboard(page);
  await run(page, "SELECT MAP(ARRAY['k'], ARRAY[1]) AS m, CAST(ROW(1, 'x') AS row(a integer, b varchar)) AS r, UUID 'b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12' AS u");
  const [[m, r, u]] = await rows(page);
  assert.equal(u, "b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12");
  assert.doesNotMatch(m, /^map\[/, `map shown as ${m}`);
  assert.doesNotMatch(r, /^\{\[/, `row shown as ${r}`);
}
