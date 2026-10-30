// Unit tests of the Users and roles view's pure helpers (src/admin/users.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  alterSpec,
  createSpec,
  flagChanges,
  formProblem,
  grantTarget,
  groupPrivileges,
  matchesPrincipal,
  membersOf,
  offeredTargets,
  parseLimit,
  principalKey,
  principalLabel,
  privilegesOn,
  sortPrincipals,
} from "../src/admin/users.ts";

const principal = (name, extra = {}) => ({
  name,
  host: null,
  kind: "user",
  flags: ["login"],
  connection_limit: null,
  valid_until: null,
  authentication: null,
  member_of: [],
  ...extra,
});

const form = (extra = {}) => ({ name: "app", host: "", flags: [], limit: "", validUntil: "", password: "", ...extra });

test("an account is told by name and host", () => {
  assert.equal(principalKey(principal("app", { host: "%" })), "app@%");
  assert.equal(principalKey(principal("app")), "app@");
  assert.equal(principalLabel(principal("app", { host: "10.%" })), "app@10.%");
  assert.equal(principalLabel(principal("app")), "app");
});

test("the filter looks at the name, the host and the roles", () => {
  const p = principal("Reporting", { host: "10.0.0.%", member_of: ["analysts"] });
  assert.ok(matchesPrincipal(p, ""));
  assert.ok(matchesPrincipal(p, "  report "));
  assert.ok(matchesPrincipal(p, "10.0"));
  assert.ok(matchesPrincipal(p, "ANALY"));
  assert.ok(!matchesPrincipal(p, "billing"));
});

test("the members of a role are those that name it", () => {
  const role = principal("analysts", { kind: "role" });
  const list = [role, principal("a", { member_of: ["analysts"] }), principal("b"), principal("c", { member_of: ["analysts", "x"] })];
  assert.deepEqual(
    membersOf(list, role).map((p) => p.name),
    ["a", "c"],
  );
});

test("the list is sorted by name, then host", () => {
  const sorted = sortPrincipals([principal("b"), principal("a", { host: "z" }), principal("a", { host: "a" })]);
  assert.deepEqual(sorted.map(principalKey), ["a@a", "a@z", "b@"]);
});

test("privileges are grouped by object, the server first and columns with their table", () => {
  const grant = (extra) => ({ kind: "table", schema: "public", object: null, column: null, privilege: "SELECT", grantable: false, ...extra });
  const groups = groupPrivileges([
    grant({ object: "orders", privilege: "UPDATE" }),
    grant({ object: "orders", column: "id", privilege: "SELECT" }),
    grant({ object: "orders", privilege: "SELECT", grantable: true }),
    grant({ kind: "schema", schema: "sales", privilege: "USAGE" }),
    grant({ kind: "server", schema: null, privilege: "PROCESS" }),
    grant({ object: "customers", privilege: "SELECT" }),
    grant({ object: "orders", privilege: "SELECT" }),
  ]);
  assert.deepEqual(
    groups.map((g) => g.label),
    ["*.*", "sales", "public.customers", "public.orders"],
  );
  const orders = groups.find((g) => g.label === "public.orders");
  assert.deepEqual(orders.privileges, [
    { name: "UPDATE", grantable: false },
    { name: "SELECT (id)", grantable: false },
    { name: "SELECT", grantable: true },
  ]);
});

const options = {
  flags: ["login"],
  targets: [
    { target: "server", privileges: [] },
    { target: "database", privileges: ["CONNECT"] },
    { target: "schema", privileges: ["USAGE", "CREATE"] },
    { target: "table", privileges: ["SELECT"] },
    { target: "all_tables", privileges: ["SELECT"] },
    { target: "all_routines", privileges: [] },
  ],
};

test("only the kinds of object with privileges are offered", () => {
  assert.deepEqual(offeredTargets(options), ["database", "schema", "table", "all_tables"]);
  assert.deepEqual(privilegesOn(options, "schema"), ["USAGE", "CREATE"]);
  assert.deepEqual(privilegesOn(options, "all_routines"), []);
});

test("a target is complete when its names are", () => {
  assert.deepEqual(grantTarget({ class: "server", schema: "", table: "" }), { kind: "server" });
  assert.deepEqual(grantTarget({ class: "table", schema: " public ", table: "orders" }), { kind: "table", schema: "public", name: "orders" });
  assert.equal(grantTarget({ class: "table", schema: "public", table: "" }), null);
  assert.equal(grantTarget({ class: "schema", schema: " ", table: "" }), null);
  assert.deepEqual(grantTarget({ class: "all_tables", schema: "s", table: "t" }), { kind: "all_tables", schema: "s" });
  // SQL Server's database is the one the session is in: it needs no name.
  assert.deepEqual(grantTarget({ class: "database", schema: "", table: "" }), { kind: "database", name: "" });
});

test("a limit is empty or a whole number", () => {
  assert.equal(parseLimit(""), null);
  assert.equal(parseLimit(" 5 "), 5);
  assert.equal(parseLimit("0"), 0);
  assert.ok(Number.isNaN(parseLimit("-1")));
  assert.ok(Number.isNaN(parseLimit("2.5")));
  assert.ok(Number.isNaN(parseLimit("many")));
});

test("flag changes are those that differ among the known ones", () => {
  assert.deepEqual(flagChanges(["login", "superuser"], ["login", "create_db"], ["login", "superuser", "create_db"]), {
    on: ["create_db"],
    off: ["superuser"],
  });
  // An attribute the database does not have is not touched.
  assert.deepEqual(flagChanges(["locked"], [], ["login"]), { on: [], off: [] });
});

test("a created user states what is on and what is off", () => {
  const spec = createSpec("user", form({ host: " % ", flags: ["login"], limit: "3", password: "pw" }), ["login", "inherit"]);
  assert.deepEqual(spec, {
    name: "app",
    host: "%",
    kind: "user",
    flags_on: ["login"],
    flags_off: ["inherit"],
    connection_limit: 3,
    valid_until: null,
    password: "pw",
  });
  // A role has no host.
  assert.equal(createSpec("role", form({ host: "%" }), []).host, null);
});

test("an altered user states only what changed", () => {
  const p = principal("app", { host: "%", flags: ["login", "inherit"], connection_limit: 5, valid_until: "2030-01-01" });
  const unchanged = alterSpec(p, form({ name: "app", flags: ["login", "inherit"], limit: "5", validUntil: "2030-01-01" }), ["login", "inherit", "superuser"]);
  assert.deepEqual(unchanged, {
    name: "app",
    host: "%",
    kind: "user",
    flags_on: [],
    flags_off: [],
    connection_limit: null,
    valid_until: null,
    password: null,
    rename_to: null,
  });
  const changed = alterSpec(p, form({ name: "web", flags: ["login", "superuser"], limit: "", validUntil: "", password: "new" }), [
    "login",
    "inherit",
    "superuser",
  ]);
  assert.deepEqual(changed.flags_on, ["superuser"]);
  assert.deepEqual(changed.flags_off, ["inherit"]);
  // Emptied fields remove the limit and the date.
  assert.equal(changed.connection_limit, -1);
  assert.equal(changed.valid_until, "");
  assert.equal(changed.password, "new");
  assert.equal(changed.rename_to, "web");
});

test("the form says what stops it", () => {
  assert.equal(formProblem(form(), ""), null);
  assert.equal(formProblem(form({ name: "  " }), ""), "name");
  assert.equal(formProblem(form({ limit: "x" }), ""), "limit");
  assert.equal(formProblem(form({ password: "a" }), "b"), "password");
  assert.equal(formProblem(form({ password: "a" }), "a"), null);
});
