// Unit tests of the index suggested by a plan (src/explain/indexSql.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { createIndexSql, filterKeys } from "../src/explain/indexSql.ts";

test("columns compared in a filter", () => {
  assert.deepEqual(filterKeys("(status = 'paid'::text)"), ["status"]);
  assert.deepEqual(filterKeys("((total > 10) AND (o.status = 'x = y') AND (total < 99))"), ["total", "status"]);
  assert.deepEqual(filterKeys('(("Created At")::date >= $1)'), ["Created At"]);
  assert.deepEqual(filterKeys("(name ~~ 'a%'::text AND deleted_at IS NULL)"), ["name", "deleted_at"]);
  assert.deepEqual(filterKeys("(a IN (1, 2) OR `b` = 3)"), ["a", "b"]);
  assert.deepEqual(filterKeys("(1 = 1)"), []);
  assert.deepEqual(filterKeys("(lower((name)::text) = 'bob'::text)"), [{ expression: "lower(name)" }]);
  assert.deepEqual(filterKeys(`(("s"."Name")::text = 'x'::text)`), ["Name"]);
  assert.deepEqual(filterKeys("(price::numeric > 3)"), ["price"]);
});

test("CREATE INDEX statement", () => {
  assert.equal(createIndexSql("postgres", "orders", ["status"]), 'CREATE INDEX orders_status_idx ON orders (status);');
  assert.equal(createIndexSql("postgres", "orders", ["Created At", "user"]), 'CREATE INDEX "orders_Created_At_user_idx" ON orders ("Created At", "user");');
  assert.equal(createIndexSql("mysql", "shop.orders", ["total"]), "CREATE INDEX orders_total_idx ON shop.orders (total);");
});

test("a function compared whole is an expression, with its column in it", () => {
  assert.deepEqual(filterKeys("(lower((email)::text) = 'x1@example.com'::text)"), [{ expression: "lower(email)" }]);
  assert.deepEqual(filterKeys("((o.id > 3) AND (date_trunc('day', o.created_at) = '2024-01-01'::timestamp))"), ["id", { expression: "date_trunc('day', created_at)" }]);
  assert.deepEqual(filterKeys("(lower((a)::text) = 'x'::text AND b = 2)"), [{ expression: "lower(a)" }, "b"]);
});

test("CREATE INDEX statement on an expression and on a table of a schema", () => {
  assert.equal(
    createIndexSql("postgres", "qa4a2e.people", [{ expression: "lower(email)" }]),
    "CREATE INDEX people_lower_email_idx ON qa4a2e.people ((lower(email)));",
  );
  assert.equal(createIndexSql("postgres", "qa4a2.items", ["kind"]), "CREATE INDEX items_kind_idx ON qa4a2.items (kind);");
});