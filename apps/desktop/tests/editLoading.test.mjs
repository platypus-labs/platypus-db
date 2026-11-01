// F2 on a table whose columns are still loading is not silently ignored (src/grid/edit/loading.ts):
// the action stays offered, so that it can say why nothing opens.
import assert from "node:assert/strict";
import { test } from "node:test";
import { offersEdit } from "../src/grid/edit/loading.ts";

test("Edit cell is offered while the columns load, and not when the table is read-only", () => {
  assert.equal(offersEdit({ editable: true, loading: false }), true);
  assert.equal(offersEdit({ editable: false, loading: true }), true);
  assert.equal(offersEdit({ editable: false, loading: false }), false);
  assert.equal(offersEdit(undefined), false);
});
