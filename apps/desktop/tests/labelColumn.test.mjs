// Unit tests of the column that describes a referenced table's rows (src/grid/edit/labelColumn.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { labelColumn } from "../src/grid/edit/labelColumn.ts";

const column = (name, data_type = "text", primary_key = false) => ({ name, data_type, primary_key });
const customers = {
  columns: [column("id", "integer", true), column("code"), column("name"), column("company"), column("score", "integer")],
};

test("without a choice, a known name wins over the first text column", () => {
  assert.equal(labelColumn(customers, "id"), "name");
  assert.equal(labelColumn({ columns: [column("id", "integer", true), column("company")] }, "id"), "company");
  assert.equal(labelColumn({ columns: [column("id", "integer", true), column("n", "integer")] }, "id"), null);
});

test("the user's choice wins when the table has that column", () => {
  assert.equal(labelColumn(customers, "id", "company"), "company");
  // Any column, even one the automatic rule skips.
  assert.equal(labelColumn(customers, "id", "score"), "score");
});

test("a choice that is the key, or no longer a column, falls back to the automatic one", () => {
  assert.equal(labelColumn(customers, "id", "id"), "name");
  assert.equal(labelColumn(customers, "id", "dropped"), "name");
  assert.equal(labelColumn(customers, "id", undefined), "name");
});
