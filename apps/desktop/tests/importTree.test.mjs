// Unit tests of the import dialog's preview (src/components/importTree.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { defaultInto, importRequest, isEmptyImport, savedPasswords, skippedIds } from "../src/components/importTree.ts";

const connection = (id, name) => ({ id, name, driver: "sql_server", host: "h", port: 1433, database: "d", user: "" });

const project = (over) => ({ name: "Vericode", folders: [], connections: [], placement: {}, skipped: [], notes: {}, project_notes: [], with_password: [], ...over });

test("a file is empty only without connections, imported or left out", () => {
  assert.equal(isEmptyImport([]), true);
  assert.equal(isEmptyImport([project({})]), true);
  const oracle = { name: "Ora", provider: "oracle", driver: "oracle_thin" };
  assert.equal(isEmptyImport([project({ skipped: [oracle] })]), false);
  assert.equal(isEmptyImport([project({ connections: [connection("a", "A")] })]), false);
});

test("a left-out connection is told by its database and driver ids", () => {
  assert.equal(skippedIds({ name: "x", provider: "oracle", driver: "oracle_thin" }), "oracle · oracle_thin");
  assert.equal(skippedIds({ name: "x", provider: "generic", driver: "" }), "generic");
  assert.equal(skippedIds({ name: "x", provider: "", driver: "" }), "?");
});

test("the request carries the ticked connections only, without notes or left-out ones", () => {
  const read = [
    project({
      connections: [connection("a", "A"), connection("b", "B")],
      skipped: [{ name: "Ora", provider: "oracle", driver: "oracle_thin" }],
      notes: { a: [{ kind: "windows_auth" }], b: [{ kind: "entra_auth" }] },
    }),
    project({ name: "Only Oracle", skipped: [{ name: "Ora", provider: "oracle", driver: "" }] }),
  ];
  const request = importRequest(read, new Set(["a"]), [{ kind: "new_project", name: "Vericode" }, { kind: "none" }], null);
  assert.equal(request.targets.length, 1);
  const [target] = request.targets;
  assert.deepEqual(
    target.project.connections.map((c) => c.id),
    ["a"],
  );
  assert.deepEqual(target.project.skipped, []);
  assert.deepEqual(target.project.notes, {});
});

test("saved passwords go along only when asked, and only for ticked connections", () => {
  const read = [
    project({
      connections: [connection("a", "A"), connection("b", "B")],
      with_password: ["a", "b"],
      project_notes: [{ kind: "saved_logins_unreadable" }],
    }),
    project({ name: "Other", connections: [connection("c", "C")] }),
  ];
  assert.equal(savedPasswords(read), 2);
  const into = [{ kind: "new_project", name: "Vericode" }, { kind: "none" }];
  const asked = importRequest(read, new Set(["a", "c"]), into, null, true);
  assert.equal(asked.passwords, true);
  assert.deepEqual(asked.targets[0].project.with_password, ["a"]);
  assert.deepEqual(asked.targets[0].project.project_notes, []);
  assert.equal(importRequest(read, new Set(["a"]), into, null).passwords, false);
});
test("a project imported again goes into the saved project of its name", () => {
  const saved = [{ id: "p1", name: "Cliente Rossi" }, { id: "p2", name: "Interno" }];
  assert.deepEqual(defaultInto(project({ name: "Cliente Rossi" }), null, saved), { kind: "existing", id: "p1" });
  assert.deepEqual(defaultInto(project({ name: " interno " }), null, saved), { kind: "existing", id: "p2" });
  assert.deepEqual(defaultInto(project({ name: "Nuovo" }), null, saved), { kind: "new_project", name: "Nuovo" });
  // Plain connections join the active project, as before.
  assert.deepEqual(defaultInto(project({ name: "" }), "p2", saved), { kind: "existing", id: "p2" });
  assert.deepEqual(defaultInto(project({ name: "" }), null, saved), { kind: "none" });
});
