// Unit tests of the completion scope (src/editor/sqlScope.ts), run by `npm test` with Node's
// own test runner; Node strips the TypeScript types when importing the module.
import assert from "node:assert/strict";
import { test } from "node:test";
import { maskSql, nameOptions, statementAt } from "../src/editor/sqlScope.ts";

const catalog = {
  default_schema: "main",
  schemas: {
    main: {
      orders: ["id", "customer_id", "total", "placed_at"],
      customers: ["id", "name", "Full Name"],
      products: ["sku", "price"],
      "My Table": ["id", "Some Col"],
    },
    sales: { invoices: ["number", "amount"] },
  },
};

/** The options at `|` in `sql`, as `kind:label` (and `@source` for unqualified columns). */
function at(sql, { explicit = false, brackets = false } = {}) {
  const pos = sql.indexOf("|");
  const r = nameOptions(sql.replace("|", ""), pos, catalog, explicit, { brackets });
  return r && r.items.map((i) => `${i.kind}:${i.label}${i.source ? `@${i.source}` : ""}`);
}
const columns = (sql, options) => (at(sql, options) ?? []).filter((o) => o.startsWith("column:"));

test("columns of the tables named after the cursor", () => {
  assert.deepEqual(columns("SELECT to| FROM orders"), [
    "column:id@orders",
    "column:customer_id@orders",
    "column:total@orders",
    "column:placed_at@orders",
  ]);
});

test("nothing typed opens the list only when asked", () => {
  assert.equal(at("SELECT | FROM orders"), null);
  assert.ok(columns("SELECT | FROM orders", { explicit: true }).length === 4);
});

test("qualified by table, alias and AS alias, ignoring case", () => {
  const ids = ["column:id", "column:customer_id", "column:total", "column:placed_at"];
  assert.deepEqual(at("SELECT orders.| FROM orders"), ids);
  assert.deepEqual(at("SELECT * FROM orders o WHERE o.|"), ids);
  assert.deepEqual(at("SELECT * FROM orders O WHERE o.|"), ids);
  assert.deepEqual(at("SELECT * FROM Orders WHERE Orders.|"), ids);
  assert.deepEqual(at("SELECT * FROM orders o JOIN customers AS c ON c.|"), ["column:id", "column:name", "column:Full Name"]);
});

test("the clauses offer only the columns in scope", () => {
  for (const sql of [
    "SELECT * FROM orders WHERE |",
    "SELECT * FROM orders WHERE id = 1 AND |",
    "SELECT * FROM orders ORDER BY |",
    "SELECT customer_id FROM orders GROUP BY |",
    "SELECT customer_id FROM orders GROUP BY customer_id HAVING |",
    "INSERT INTO orders (|",
    "UPDATE orders SET |",
    "DELETE FROM orders WHERE |",
  ]) {
    const cols = columns(sql, { explicit: true });
    assert.ok(cols.includes("column:total@orders"), sql);
    assert.ok(!cols.some((c) => c.includes("price")), sql);
  }
});

test("after FROM and JOIN: tables and schemas, no columns", () => {
  assert.deepEqual(at("SELECT * FROM o|"), [
    "table:orders",
    "table:customers",
    "table:products",
    "table:My Table",
    "schema:main",
    "schema:sales",
  ]);
  assert.deepEqual(columns("SELECT * FROM orders JOIN |", { explicit: true }), []);
  assert.deepEqual(columns("SELECT * FROM orders, |", { explicit: true }), []);
  assert.deepEqual(at("SELECT * FROM sales.|"), ["table:invoices"]);
  assert.deepEqual(at("SELECT * FROM SALES.inv|"), ["table:invoices"]);
});

test("quoted names", () => {
  assert.deepEqual(at('SELECT * FROM "My Table" m WHERE m.|'), ["column:id", "column:Some Col"]);
  assert.deepEqual(at('SELECT "My Table".| FROM "My Table"'), ["column:id", "column:Some Col"]);
  const pos = 'SELECT * FROM "My'.length;
  const r = nameOptions('SELECT * FROM "My"', pos, catalog, false);
  assert.equal(r.quote, '"');
  assert.equal(r.from, "SELECT * FROM ".length);
  assert.deepEqual(at("SELECT * FROM [My| ", { brackets: true })?.includes("table:My Table"), true);
});

test("no names where an alias is written", () => {
  assert.equal(at("SELECT * FROM orders o|"), null);
  assert.equal(at("SELECT total AS t|"), null);
  assert.equal(at("INSERT INTO orders v|"), null);
});

test("CTE columns, from its select list or its column list", () => {
  assert.deepEqual(at("WITH x AS (SELECT id, total AS amount FROM orders) SELECT x.| FROM x"), ["column:id", "column:amount"]);
  assert.deepEqual(at("WITH x (a, b) AS (SELECT 1, 2) SELECT x.| FROM x"), ["column:a", "column:b"]);
  assert.deepEqual(at("WITH x AS (SELECT * FROM products) SELECT x.| FROM x"), ["column:sku", "column:price"]);
  assert.deepEqual(columns("WITH x AS (SELECT id, total AS amount FROM orders) SELECT am| FROM x"), [
    "column:id@x",
    "column:amount@x",
  ]);
  // Inside the CTE body, its own tables.
  assert.deepEqual(columns("WITH x AS (SELECT pr| FROM products) SELECT * FROM orders"), [
    "column:sku@products",
    "column:price@products",
  ]);
  assert.deepEqual(at("WITH a AS (SELECT 1 AS n), b AS (SELECT n FROM a) SELECT b.| FROM b"), ["column:n"]);
});

test("subqueries in FROM with an alias", () => {
  assert.deepEqual(at("SELECT s.| FROM (SELECT sku, price AS p, count(*) n, a.b FROM products) s"), [
    "column:sku",
    "column:p",
    "column:n",
    "column:b",
  ]);
  // Their tables stay inside them.
  assert.deepEqual(columns("SELECT p| FROM (SELECT sku FROM products) AS s"), ["column:sku@s"]);
  // A subquery in WHERE sees the tables around it.
  const inner = columns("SELECT * FROM orders WHERE customer_id IN (SELECT i| FROM customers)");
  assert.ok(inner.includes("column:id@customers") && inner.includes("column:total@orders"));
  // ...but they do not see its tables.
  assert.ok(!columns("SELECT n| FROM orders WHERE customer_id IN (SELECT id FROM customers)").some((c) => c.includes("customers")));
});

test("strings, comments and quoted names do not confuse the scope", () => {
  assert.deepEqual(columns("SELECT pr| FROM orders WHERE x = 'FROM products' -- JOIN products\n"), [
    "column:id@orders",
    "column:customer_id@orders",
    "column:total@orders",
    "column:placed_at@orders",
  ]);
  assert.equal(maskSql("a 'b;c' -- d\ne /* f */ \"g'h\""), "a       " + "    \ne " + "        " + "\"g'h\"");
  assert.equal(maskSql("$$x;y$$ z"), "        z");
});

test("the statement under the cursor, not its neighbours", () => {
  const doc = "SELECT * FROM products;\nSELECT 'a;b' FROM orders WHERE x;\nSELECT 3";
  const pos = doc.indexOf("WHERE");
  const { from, to } = statementAt(doc, pos);
  assert.equal(doc.slice(from, to), "\nSELECT 'a;b' FROM orders WHERE x");
  assert.deepEqual(statementAt("SELECT 1;", 9), { from: 9, to: 9 });
  assert.deepEqual(statementAt("SELECT 1", 3), { from: 0, to: 8 });
});

test("a table not in the statement still completes after its name", () => {
  assert.deepEqual(at("SELECT customers.|"), ["column:id", "column:name", "column:Full Name"]);
  assert.equal(at("SELECT nothing.|"), null);
});
