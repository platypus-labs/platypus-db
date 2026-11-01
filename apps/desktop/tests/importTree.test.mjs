// Unit tests of the import dialog's preview (src/components/importTree.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { defaultInto, importRequest, isEmptyImport, replacements, savedPasswords, savedTwin, skippedIds } from "../src/components/importTree.ts";

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

test("a ticked connection like a saved one replaces it, each saved one once", () => {
  const saved = [{ ...connection("s1", "Saved"), user: "sa" }, connection("s2", "Other")];
  saved[1].database = "other";
  const read = [
    project({ connections: [{ ...connection("a", "A"), user: "sa" }, { ...connection("b", "B"), user: "sa" }, connection("c", "C")] }),
    project({ name: "More", connections: [{ ...connection("d", "D"), database: "other" }] }),
  ];
  assert.equal(savedTwin(read[0].connections[0], saved)?.id, "s1");
  assert.equal(savedTwin(read[0].connections[2], saved), undefined, "another user");
  // `b` matches `s1` too, but `a` took it; `d` is not ticked.
  const replace = replacements(read, new Set(["a", "b", "c"]), saved);
  assert.deepEqual(replace, { a: "s1" });
  assert.deepEqual(replacements(read, new Set(["b", "d"]), saved), { b: "s1", d: "s2" });
  assert.deepEqual(importRequest(read, new Set(["a"]), [{ kind: "none" }, { kind: "none" }], null, false, replace).replace, { a: "s1" });
  assert.deepEqual(importRequest(read, new Set(["a"]), [{ kind: "none" }, { kind: "none" }], null).replace, {});
});

test("a connection through another SSH tunnel is not the saved one", () => {
  const bastion = (host, jump = null) => ({ host, port: 22, user: "ops", auth: "password", key_path: null, secret_id: "x", jump });
  const saved = [{ ...connection("s", "Saved"), ssh: bastion("b", bastion("j")) }];
  assert.equal(savedTwin({ ...connection("a", "A"), ssh: bastion("b", bastion("j")) }, saved)?.id, "s");
  assert.equal(savedTwin({ ...connection("a", "A"), ssh: bastion("b") }, saved), undefined, "a hop less");
  assert.equal(savedTwin({ ...connection("a", "A"), ssh: bastion("evil", bastion("j")) }, saved), undefined);
  assert.equal(savedTwin(connection("a", "A"), saved), undefined, "direct");
});
