// Unit tests of the index suggested by a plan (src/explain/indexSql.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { createIndexSql, filterColumns } from "../src/explain/indexSql.ts";

test("columns compared in a filter", () => {
  assert.deepEqual(filterColumns("(status = 'paid'::text)"), ["status"]);
  assert.deepEqual(filterColumns("((total > 10) AND (o.status = 'x = y') AND (total < 99))"), ["total", "status"]);
  assert.deepEqual(filterColumns('(("Created At")::date >= $1)'), ["Created At"]);
  assert.deepEqual(filterColumns("(name ~~ 'a%'::text AND deleted_at IS NULL)"), ["name", "deleted_at"]);
  assert.deepEqual(filterColumns("(a IN (1, 2) OR `b` = 3)"), ["a", "b"]);
  assert.deepEqual(filterColumns("(1 = 1)"), []);
});

test("CREATE INDEX statement", () => {
  assert.equal(createIndexSql("postgres", "orders", ["status"]), 'CREATE INDEX orders_status_idx ON orders (status);');
  assert.equal(createIndexSql("postgres", "orders", ["Created At", "user"]), 'CREATE INDEX "orders_Created_At_user_idx" ON orders ("Created At", "user");');
  assert.equal(createIndexSql("mysql", "shop.orders", ["total"]), "CREATE INDEX orders_total_idx ON shop.orders (total);");
});
