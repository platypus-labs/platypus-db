// Unit tests of how rows an agent proposed become the data editor's pending changes
// (src/grid/edit/proposed.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { columnRule } from "../src/grid/edit/massEdit.ts";
import { applyProposal, columnIndex, neededColumns } from "../src/grid/edit/proposed.ts";
import { EMPTY, rowRef } from "../src/grid/edit/state.ts";

const info = (name, db_type, extra = {}) => ({ name, db_type, numeric: false, binary: false, ...extra });
const meta = (name, data_type, extra = {}) => ({
  name,
  data_type,
  nullable: true,
  default: null,
  primary_key: false,
  comment: null,
  enum_values: [],
  fields: [],
  ...extra,
});
const value = (text) => ({ kind: "value", text });

/** id integer (key), name text, score real, ghost (database-filled), expr (not the table's). */
const columns = [
  { name: "id", rule: columnRule(info("id", "integer"), meta("id", "integer", { nullable: false, primary_key: true })) },
  { name: "name", rule: columnRule(info("name", "text"), meta("name", "text")) },
  { name: "score", rule: columnRule(info("score", "real", { numeric: true }), meta("score", "real")) },
  { name: "ghost", rule: undefined, fixed: "database" },
  { name: "expr", rule: undefined, fixed: "not_table" },
];

/** Three loaded rows. */
const ROWS = [
  ["1", "Ada", "1.5", "g", "x"],
  ["2", "Bob", null, "g", "x"],
  ["03", "Cy", "2.50", "g", "x"],
];
const loaded = (row, col) => ROWS[row]?.[col];
const proposal = (extra) => ({ schema: "main", table: "t", inserts: [], updates: [], deletes: [], ...extra });
const apply = (p, state = EMPTY) => applyProposal(state, p, columns, ROWS.length, loaded);

test("finds columns by name, and says which ones it needs", () => {
  assert.equal(columnIndex(columns, "name"), 1);
  assert.equal(columnIndex(columns, "NAME"), 1);
  assert.equal(columnIndex(columns, "nope"), -1);
  const p = proposal({
    updates: [{ key: [["id", "1"]], set: [["score", "9"], ["nope", "1"]] }],
    deletes: [[["id", "2"]]],
  });
  assert.deepEqual(neededColumns(p, columns), [0, 2]);
});

test("updates, deletes and inserts become pending changes of the right rows", () => {
  const p = proposal({
    updates: [{ key: [["id", "2"]], set: [["name", "Bobby"], ["score", null]] }],
    deletes: [[["id", "1"]]],
    inserts: [[["id", "9"], ["name", "Dee"]], [["name", "Eve"]]],
  });
  const out = apply(p);
  assert.deepEqual(out.skipped, []);
  assert.equal(out.applied, 4);
  // The result's rows keep their numbers; the two new rows are on top, in order.
  assert.deepEqual([...out.state.deletes], [0]);
  assert.deepEqual([...out.state.updates.get(1)], [[1, value("Bobby")]]);
  assert.equal(out.state.inserts.length, 2);
  assert.deepEqual([...out.state.inserts[0]], [[0, value("9")], [1, value("Dee")]]);
  assert.deepEqual([...out.state.inserts[1]], [[1, value("Eve")]]);
  assert.deepEqual(rowRef(out.state, 2), { row: 0 });
});

test("a value equal to the loaded one is dropped, and numbers compare as numbers", () => {
  const out = apply(
    proposal({
      updates: [
        { key: [["id", "3"]], set: [["name", "Cy"], ["score", "2.5"]] },
        { key: [["id", "1"]], set: [["score", "1.50"]] },
      ],
    }),
  );
  assert.deepEqual(out.skipped, []);
  assert.equal(out.applied, 0);
  assert.equal(out.state.updates.size, 0);
});

test("a key is matched by the cells as loaded, column by column and by name", () => {
  // '03' is loaded, the proposal says 3; the composite key is given in another order.
  const out = apply(proposal({ deletes: [[["id", "3"]]], updates: [{ key: [["name", "Ada"], ["id", "1"]], set: [["name", "A"]] }] }));
  assert.deepEqual(out.skipped, []);
  assert.deepEqual([...out.state.deletes], [2]);
  assert.deepEqual([...out.state.updates.get(0)], [[1, value("A")]]);
  // A key matching one column only does not match a row.
  const half = apply(proposal({ deletes: [[["id", "1"], ["name", "Bob"]]] }));
  assert.deepEqual(half.skipped.map((s) => s.reason), ["no_row"]);
});

test("nothing is dropped silently: what cannot be applied is listed, the rest applied", () => {
  const p = proposal({
    updates: [
      { key: [["id", "99"]], set: [["name", "x"]] },
      { key: [["id", "1"]], set: [["name", "Ada L."], ["nope", "1"]] },
      { key: [["id", "2"]], set: [["ghost", "y"]] },
      { key: [["id", "2"]], set: [["expr", "y"]] },
      { key: [["id", "2"]], set: [["id", "abc"]] },
      { key: [["id", "2"]], set: [["name", "ok"]] },
    ],
    deletes: [[["id", "7"]], [["nope", "1"]]],
    inserts: [[["id", "x"]], [["name", "fine"]]],
  });
  const out = apply(p);
  assert.deepEqual(
    out.skipped.map((s) => [s.kind, s.index, s.reason, s.column, s.problem]),
    [
      ["update", 0, "no_row", undefined, undefined],
      ["update", 1, "missing_column", "nope", undefined],
      ["update", 2, "database", "ghost", undefined],
      ["update", 3, "not_table", "expr", undefined],
      ["update", 4, "value", "id", "integer"],
      ["delete", 0, "no_row", undefined, undefined],
      ["delete", 1, "missing_column", "nope", undefined],
      ["insert", 0, "value", "id", "integer"],
    ],
  );
  // An update with one bad cell is not half applied.
  assert.equal(out.state.updates.get(0), undefined);
  assert.deepEqual([...out.state.updates.get(1)], [[1, value("ok")]]);
  assert.equal(out.state.inserts.length, 1);
  assert.equal(out.applied, 2);
});

test("hostile text is only ever a value to compare or write", () => {
  const evil = "x'; DROP TABLE t; --";
  const out = apply(
    proposal({
      updates: [{ key: [["id", "1"]], set: [["name", evil]] }],
      deletes: [[["id", evil]]],
      inserts: [[["name", evil]]],
    }),
  );
  assert.deepEqual(out.skipped.map((s) => [s.kind, s.reason]), [["delete", "no_row"]]);
  assert.deepEqual([...out.state.updates.get(0)], [[1, value(evil)]]);
  assert.deepEqual([...out.state.inserts[0]], [[1, value(evil)]]);
});

test("the proposal goes on top of edits already pending, the rows keep their place", () => {
  const first = apply(proposal({ inserts: [[["name", "one"]]] }));
  const second = apply(
    proposal({ updates: [{ key: [["id", "2"]], set: [["name", "B"]] }], inserts: [[["name", "two"]]] }),
    first.state,
  );
  assert.equal(second.state.inserts.length, 2);
  assert.deepEqual([...second.state.inserts[0]], [[1, value("two")]]);
  assert.deepEqual([...second.state.updates.get(1)], [[1, value("B")]]);
});
