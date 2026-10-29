// Unit tests of what the dialogs keep in a transfer profile and take back (src/transfer/profiles.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  changedKeys,
  createsTables,
  existingTable,
  exportsFiles,
  mappingFor,
  missingColumns,
  missingConnections,
  newColumnsFor,
  profileConnections,
  profilesOf,
  splitTables,
  usesConnection,
} from "../src/transfer/profiles.ts";

const exportQuery = { kind: "export_query", connection_id: "a", database: null, sql: "select 1", path: "/o.csv", options: { format: "csv", delimiter: ",", header: true } };
const exportTables = { kind: "export_tables", connection_id: "b", database: "db", tables: [{ schema: null, name: "t" }], dir: "/o", options: { format: "json", delimiter: ",", header: true } };
const importFile = { kind: "import_file", connection_id: "c", database: null, path: "/in.csv", plan: { source_table: "in", schema: null, table: "t", create: false, columns: [] }, options: { encoding: null, skip_bad_lines: false } };
const transfer = { kind: "transfer_table", source_connection_id: "a", source_database: null, target_connection_id: "d", target_database: null, plan: importFile.plan };
const migrate = { kind: "migrate", source_connection_id: "a", source_database: null, target_connection_id: "d", target_database: null, source_schema: "public", target_schema: "public", tables: ["t"], objects: true };

test("each kind names the connections it uses, the first being where its data is", () => {
  assert.deepEqual(profileConnections(exportQuery), ["a"]);
  assert.deepEqual(profileConnections(exportTables), ["b"]);
  assert.deepEqual(profileConnections(importFile), ["c"]);
  assert.deepEqual(profileConnections(transfer), ["a", "d"]);
  assert.deepEqual(profileConnections(migrate), ["a", "d"]);
});

test("a connection that is not saved any more is found", () => {
  assert.deepEqual(missingConnections(migrate, ["a", "d"]), []);
  assert.deepEqual(missingConnections(migrate, ["a"]), ["d"]);
  assert.deepEqual(missingConnections(exportQuery, []), ["a"]);
});

test("only exports write a file or a folder", () => {
  assert.deepEqual([exportQuery, exportTables, importFile, transfer, migrate].map(exportsFiles), [true, true, false, false, false]);
});

test("a connection's list holds the profiles that use it, by name", () => {
  const profileOf = (name, task, id) => ({ id, name, task });
  const all = [profileOf("Zeta", migrate, "1"), profileOf("Alpha", exportQuery, "2"), profileOf("Beta", importFile, "3")];
  assert.deepEqual(profilesOf(all).map((p) => p.name), ["Alpha", "Beta", "Zeta"]);
  assert.deepEqual(profilesOf(all, "d").map((p) => p.name), ["Zeta"]);
  assert.ok(usesConnection(all[1], "a"));
  assert.ok(!usesConnection(all[1], "d"));
});

const filling = {
  source_table: "in",
  schema: null,
  table: "t",
  create: false,
  mode: "upsert",
  keys: ["id"],
  columns: [
    { source: "ID", target: "id", data_type: null },
    { source: "full_name", target: "Name", data_type: null },
  ],
};

test("an import into a table fills the same columns from the same sources again", () => {
  assert.deepEqual(mappingFor(["id", "name", "note"], filling), ["ID", "full_name", ""]);
  assert.deepEqual(mappingFor([], filling), []);
});

test("a new table gets the saved names and types, and leaves out the columns it left out", () => {
  const defaults = [
    { include: true, target: "a", type: "text" },
    { include: true, target: "b", type: "text" },
    { include: true, target: "c", type: "text" },
  ];
  const created = { ...filling, create: true, columns: [{ source: "a", target: "alpha", data_type: "varchar(9)" }, { source: "c", target: "c", data_type: null }] };
  assert.deepEqual(newColumnsFor(["a", "b", "c"], created, defaults), [
    { include: true, target: "alpha", type: "varchar(9)" },
    { include: false, target: "b", type: "text" },
    { include: true, target: "c", type: "text" },
  ]);
  assert.equal(newColumnsFor(["a"], filling, defaults), defaults, "a plan into a table has no new columns");
});

test("columns the plan names that the file or table lost are found", () => {
  assert.deepEqual(missingColumns(filling, ["ID", "full_name"], ["id", "name"]), { sources: [], targets: [] });
  assert.deepEqual(missingColumns(filling, ["ID"], ["id"]), { sources: ["full_name"], targets: ["Name"] });
  assert.deepEqual(missingColumns({ ...filling, create: true }, ["ID"], ["id"]).targets, [], "a new table has no columns to lose");
  assert.deepEqual(missingColumns(filling, ["ID", "full_name"], null).targets, []);
});

test("a changed primary key is noticed for an upsert only", () => {
  assert.ok(!changedKeys(filling, ["id"]));
  assert.ok(changedKeys(filling, ["id", "tenant"]));
  assert.ok(!changedKeys({ ...filling, mode: "append", keys: [] }, ["id"]));
});

test("tables that are gone are split from those found", () => {
  const tables = [{ name: "a" }, { name: "b" }, { name: "c" }];
  assert.deepEqual(splitTables(tables, ["a", "c", "z"]), { kept: [{ name: "a" }, { name: "c" }], gone: [{ name: "b" }] });
});

test("a plan into an existing table names it; one that creates its table names none", () => {
  assert.equal(existingTable(importFile.plan), "t");
  assert.equal(existingTable({ ...importFile.plan, create: true }), null);
});

test("an import or copy into a new table, and a migration, create tables on each run", () => {
  const creating = { ...importFile.plan, create: true };
  assert.equal(createsTables(importFile), false);
  assert.equal(createsTables({ ...importFile, plan: creating }), true);
  assert.equal(createsTables({ ...transfer, plan: creating }), true);
  assert.equal(createsTables(migrate), true);
  assert.equal(createsTables(exportQuery), false);
  assert.equal(createsTables(exportTables), false);
});
