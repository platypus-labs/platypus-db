// Unit tests of the edit mode of the ER diagram (src/erd/edits.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { designProblem, dragKey, drawnTables, emptyEdits, pendingCount, restoreEdits, withDesign, withDropped, withRenamedNew, withoutDesign, withoutDrop } from "../src/erd/edits.ts";

const column = (name, primary_key = false) => ({ original: null, name, data_type: "INTEGER", nullable: !primary_key, default: null, primary_key, comment: null });
const design = (name, columns = [column("id", true)], foreign_keys = []) => ({ name, columns, indexes: [], foreign_keys, constraints: [] });
const read = (name, columns = [column("id", true)], foreign_keys = []) => ({ name, comment: null, error: null, details: { columns, foreign_keys, indexes: [], constraints: [], referenced_by: [] } });
const key = (name, ref_table) => ({ original: null, name, columns: ["x"], ref_schema: "main", ref_table, ref_columns: ["id"], on_delete: null, on_update: null });

test("without edits the tables are drawn from the database", () => {
  const drawn = drawnTables([read("a")], null);
  assert.deepEqual(drawn.map((d) => [d.name, d.state]), [["a", "plain"]]);
});

test("edited tables are drawn from their design, new ones after the others, dropped ones marked", () => {
  const edits = { designs: [design("a", [column("id", true), column("note")]), design("fresh")], dropped: ["b"] };
  const drawn = drawnTables([read("a"), read("b"), read("c")], edits);
  assert.deepEqual(drawn.map((d) => [d.name, d.state]), [["a", "changed"], ["b", "dropped"], ["c", "plain"], ["fresh", "new"]]);
  assert.deepEqual(drawn[0].columns.map((c) => c.name), ["id", "note"]);
});

test("pending changes count designs and drops", () => {
  assert.equal(pendingCount(emptyEdits()), 0);
  assert.equal(pendingCount({ designs: [design("a")], dropped: ["b", "c"] }), 3);
});

test("a design needs a unique name, and columns with names and types", () => {
  assert.equal(designProblem(design(" "), []), "name");
  assert.equal(designProblem(design("Users"), ["users"]), "duplicate");
  assert.equal(designProblem(design("t", []), []), "columns");
  assert.equal(designProblem(design("t", [column("")]), []), "column_name");
  assert.equal(designProblem(design("t", [{ ...column("a"), data_type: " " }]), []), "column_type");
  assert.equal(designProblem(design("t", [column("a"), column("A")]), []), "duplicate_column");
  assert.equal(designProblem(design("t"), ["u"]), null);
});

test("saving a design replaces the one of the same name, and keeps the table", () => {
  const edits = withDesign({ designs: [design("a")], dropped: ["b"] }, design("b"));
  assert.deepEqual(edits.dropped, []);
  assert.deepEqual(edits.designs.map((d) => d.name), ["a", "b"]);
  const again = withDesign(edits, design("a", [column("id", true), column("n")]));
  assert.equal(again.designs.length, 2);
  assert.equal(again.designs[0].columns.length, 2);
});

test("dropping a table removes its design and the keys other designs have to it", () => {
  const edits = withDropped({ designs: [design("a", [column("id", true)], [key("fk", "b")]), design("b")], dropped: [] }, "b");
  assert.deepEqual(edits.dropped, ["b"]);
  assert.deepEqual(edits.designs.map((d) => d.name), ["a"]);
  assert.equal(edits.designs[0].foreign_keys.length, 0);
  assert.deepEqual(withoutDrop(edits, "b").dropped, []);
});

test("removing a new table takes the keys to it away; reverting a changed one keeps them", () => {
  const edits = { designs: [design("a", [column("id", true)], [key("fk", "b")]), design("b")], dropped: [] };
  assert.equal(withoutDesign(edits, "b", true).designs[0].foreign_keys.length, 0);
  assert.equal(withoutDesign(edits, "a", false).designs.length, 1);
});

test("renaming a new table renames the keys to it", () => {
  const edits = withRenamedNew({ designs: [design("a", [column("id", true)], [key("fk", "b")]), design("b")], dropped: [] }, "b", design("c"));
  assert.deepEqual(edits.designs.map((d) => d.name), ["a", "c"]);
  assert.equal(edits.designs[0].foreign_keys[0].ref_table, "c");
});

test("a drag makes a key to the single column primary key", () => {
  const target = drawnTables([read("parent")], null)[0];
  const made = dragKey(design("child", [column("id", true), column("parent_id")]), "parent_id", target, "main");
  assert.deepEqual(made.key, { original: null, name: "fk_child_parent_id", columns: ["parent_id"], ref_schema: "main", ref_table: "parent", ref_columns: ["id"], on_delete: null, on_update: null });
});

test("a drag onto a table without a single column key says why", () => {
  const source = design("child", [column("id", true), column("p")]);
  const none = drawnTables([read("loose", [column("v")])], null)[0];
  assert.equal(dragKey(source, "p", none, "main").problem, "no_key");
  const two = drawnTables([read("pair", [column("a", true), column("b", true)])], null)[0];
  assert.equal(dragKey(source, "p", two, "main").problem, "composite_key");
});

test("the same key twice is refused, a second key to the same table gets another name", () => {
  const target = drawnTables([read("parent")], null)[0];
  const first = dragKey(design("child"), "p", target, "main").key;
  assert.equal(dragKey(design("child", [column("id", true)], [first]), "p", target, "main").problem, "exists");
  const other = dragKey(design("child", [column("id", true)], [first]), "q", target, "main").key;
  assert.equal(other.name, "fk_child_q");
  const clash = dragKey(design("child", [column("id", true)], [{ ...first, columns: ["z"] }]), "p", target, "main").key;
  assert.equal(clash.name, "fk_child_p_2");
});

test("saved edits are read back, the shapes that are not edits dropped", () => {
  assert.equal(restoreEdits(undefined), null);
  assert.equal(restoreEdits("x"), null);
  assert.equal(restoreEdits([]), null);
  const saved = { designs: [design("a"), { name: "broken" }, 3], dropped: ["b", 4] };
  assert.deepEqual(restoreEdits(saved), { designs: [design("a")], dropped: ["b"] });
  assert.deepEqual(restoreEdits({}), { designs: [], dropped: [] });
});
