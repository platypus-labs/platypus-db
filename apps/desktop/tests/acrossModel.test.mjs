// Unit tests of the table names and first text of "Query across databases" (src/analysis/acrossModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { assignNames, connectionTitle, defaultName, initialSql, nameProblems, uniqueName } from "../src/analysis/acrossModel.ts";

test("the default name is the connection and the table in lower case", () => {
  assert.equal(defaultName("PG Prod", "Orders"), "pg_prod_orders");
  assert.equal(defaultName("my-sql", "order items"), "my_sql_order_items");
  assert.equal(defaultName("", "!!"), "table");
});

test("a taken name gets the next number, ignoring case", () => {
  const taken = new Set(["a_t"]);
  assert.equal(uniqueName("A_t", taken), "A_t_2");
  assert.equal(uniqueName("a_t", taken), "a_t_3");
});

test("typed names stay and the others avoid them", () => {
  const row = (connectionName, table, edited = false, name = "") => ({ connectionName, table, edited, name });
  const names = assignNames([row("a", "t"), row("a", "t"), row("b", "u", true, "A_T")]);
  assert.deepEqual(names, ["a_t_2", "a_t_3", "A_T"]);
});

test("empty and repeated names are problems", () => {
  assert.deepEqual(nameProblems(["a", " ", "A", "b"]), [null, "empty", "duplicate", null]);
});

test("the first text lists the tables and queries the first", () => {
  const sql = initialSql([
    { name: "pg_orders", columns: ["id", "customer"], rows: 3, cut: false },
    { name: "My Customers", columns: ["id"], rows: 2, cut: false },
  ]);
  assert.equal(sql.split("\n").at(-1), "SELECT * FROM pg_orders LIMIT 100");
  assert.ok(sql.includes("--   pg_orders(id, customer)"));
  assert.ok(initialSql([{ name: "My Customers", columns: [], rows: 0, cut: false }]).endsWith('"My Customers" LIMIT 100'));
  assert.equal(connectionTitle(["pg_orders", "my_customers"]), "Across: pg_orders, my_customers");
});
