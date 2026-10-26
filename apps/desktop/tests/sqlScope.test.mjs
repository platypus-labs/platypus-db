// Unit tests of the completion scope (src/editor/sqlScope.ts), run by `npm test` with Node's
// own test runner; Node strips the TypeScript types when importing the module.
import assert from "node:assert/strict";
import { test } from "node:test";
import { maskSql, nameOptions, scopeOptions, statementAt } from "../src/editor/sqlScope.ts";

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
  assert.equal(maskSql("$$x;y$$ z", scopeOptions("postgres")), "        z");
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

// Fix round 1: the repros of the review.

const mysql = scopeOptions("mysql");
const tsql = scopeOptions("sqlserver");
const optionsAt = (sql, options) => {
  const pos = sql.indexOf("|");
  return nameOptions(sql.replace("|", ""), pos, catalog, false, options);
};
const labels = (r) => r && r.items.map((i) => `${i.kind}:${i.label}`);

test("a backslash-escaped quote in a MySQL string does not mask the rest of the script", () => {
  const doc = "INSERT INTO customers VALUES (1, 'O\\'Brien');\nSELECT to| FROM orders";
  const pos = doc.indexOf("|");
  const text = doc.replace("|", "");
  const { from, to } = statementAt(text, pos, mysql);
  assert.equal(text.slice(from, to), "\nSELECT to FROM orders");
  const r = nameOptions(text.slice(from, to), pos - from, catalog, false, mysql);
  assert.ok(labels(r).includes("column:total"));
  // Elsewhere a backslash is just a character: `'a\'` is a whole string.
  assert.equal(maskSql("'a\\' x"), "     x");
  assert.equal(maskSql("'a\\' x'", mysql), "       ");
  // In MySQL "…" is a string: its `;` splits nothing.
  assert.equal(statementAt('SELECT "a;b" FROM t', 3, mysql).to, 19);
});

test("a PostgreSQL E'…' string ends after its backslash-escaped quotes", () => {
  const pg = scopeOptions("postgres");
  const doc = "SELECT E'it\\'s' AS s;\nSELECT to| FROM orders";
  const pos = doc.indexOf("|");
  const text = doc.replace("|", "");
  const { from, to } = statementAt(text, pos, pg);
  assert.equal(text.slice(from, to), "\nSELECT to FROM orders");
  assert.ok(labels(nameOptions(text.slice(from, to), pos - from, catalog, false, pg)).includes("column:total"));
  assert.equal(maskSql("e'a\\'b' x", pg), "        x");
  // Only as a prefix of its own: `name'…'` and a plain '…' keep `\` as a character.
  assert.equal(maskSql("xe'a\\' y", pg), "xe     y");
  assert.equal(maskSql("'a\\' y", pg), "     y");
  // Elsewhere E'…' is a name followed by a string.
  assert.equal(maskSql("E'a\\' y"), "E     y");
});

test("the quote the editor closed after the cursor does not swallow FROM", () => {
  assert.ok(labels(optionsAt('SELECT "Fu|" FROM customers')).includes("column:Full Name"));
  assert.deepEqual(labels(optionsAt('SELECT c."Fu|" FROM customers c')), ["column:id", "column:name", "column:Full Name"]);
  assert.deepEqual(labels(optionsAt('SELECT "c"."Fu|" FROM customers c')), ["column:id", "column:name", "column:Full Name"]);
  assert.ok(labels(optionsAt("SELECT `Fu|` FROM customers", mysql)).includes("column:Full Name"));
  const r = optionsAt('SELECT c."Fu|" FROM customers c');
  assert.equal(r.quote, '"');
  assert.equal(r.from, 'SELECT c.'.length);
});

test("SQL Server brackets after other bracketed names", () => {
  const r = optionsAt("SELECT [a], [Fu| FROM customers", tsql);
  assert.equal(r.quote, "[");
  assert.ok(labels(r).includes("column:Full Name"));
  const q = optionsAt("SELECT [c].[Fu|] FROM customers c", tsql);
  assert.equal(q.quote, "[");
  assert.equal(q.from, "SELECT [c].".length);
  assert.deepEqual(labels(q), ["column:id", "column:name", "column:Full Name"]);
  assert.deepEqual(labels(optionsAt("SELECT * FROM [sales].[in|]", tsql)), ["table:invoices"]);
  // A quote inside a bracketed name opens no string.
  assert.ok(labels(optionsAt("SELECT * FROM customers WHERE [Customer's] = 1 AND na|", tsql)).includes("column:name"));
});

test("USING names tables in DELETE and MERGE only", () => {
  const sources = (sql) => (at(sql, { explicit: true }) ?? []).filter((o) => o.startsWith("source:"));
  // DDL: an access method, a conversion expression.
  assert.deepEqual(sources("CREATE TABLE t (a int, |) USING heap"), []);
  assert.deepEqual(sources("ALTER TABLE orders ALTER COLUMN | TYPE bigint USING total::bigint"), []);
  assert.deepEqual(sources("CREATE INDEX i ON orders USING gin (|)"), []);
  // JOIN … USING (column) names a column, never a table.
  assert.ok(!columns("SELECT * FROM orders o JOIN customers c USING (id) WHERE |", { explicit: true }).some((c) => c.endsWith("@id")));
  // DELETE and MERGE: every table of the list, a subquery too.
  const del = columns("DELETE FROM orders USING customers c, products WHERE |", { explicit: true });
  assert.ok(del.includes("column:name@c") && del.includes("column:sku@products"));
  assert.ok(columns("MERGE INTO orders o USING customers s ON s.id = o.customer_id WHEN MATCHED THEN UPDATE SET total = na|").includes("column:name@s"));
  assert.deepEqual(at("MERGE INTO orders o USING (SELECT id, name FROM customers) s ON s.|"), ["column:id", "column:name"]);
  assert.ok(columns("WITH x AS (SELECT 1 n) DELETE FROM orders USING x WHERE |", { explicit: true }).includes("column:n@x"));
});

test("a column name several tables have gets their qualifier, as written", () => {
  const itemsAt = (sql, explicit = false) =>
    nameOptions(sql.replace("|", ""), sql.indexOf("|"), catalog, explicit)?.items ?? [];
  /** The columns as accepting them inserts: `qualifier.label`, else the label. */
  const inserted = (sql) =>
    itemsAt(sql)
      .filter((i) => i.kind === "column")
      .map((i) => (i.qualifier ? `${i.qualifier}.${i.label}` : i.label));
  const join = inserted("SELECT i| FROM orders o JOIN customers c ON c.id = o.customer_id");
  assert.deepEqual(join, ["o.id", "customer_id", "total", "placed_at", "c.id", "name", "Full Name"]);
  assert.ok(inserted("SELECT i| FROM orders JOIN customers ON customers.id = orders.customer_id").includes("customers.id"));
  assert.deepEqual(inserted('SELECT i| FROM orders O, customers "C"').filter((c) => c.endsWith(".id")), ["O.id", '"C".id']);
  // One table: nothing changes.
  assert.deepEqual(inserted("SELECT i| FROM orders"), ["id", "customer_id", "total", "placed_at"]);
  // Lists of target columns take bare names.
  const bare = (sql) => {
    const items = itemsAt(sql, true);
    assert.ok(items.some((i) => i.label === "id"), sql);
    return !items.some((i) => i.qualifier);
  };
  assert.ok(bare("INSERT INTO orders (|) SELECT id FROM customers"));
  assert.ok(bare("INSERT INTO main.orders (total, |) SELECT id FROM customers"));
  assert.ok(bare("UPDATE orders SET | FROM customers WHERE customers.id = orders.customer_id"));
  assert.ok(bare("UPDATE orders SET total = 1, | FROM customers"));
  assert.ok(bare("SELECT * FROM orders JOIN customers USING (|)"));
  // ...but their values do not.
  assert.ok(!bare("UPDATE orders SET total = i| FROM customers"));
});

test("a CTE completes after its name before FROM is written", () => {
  assert.deepEqual(at("WITH x AS (SELECT id, total AS amount FROM orders) SELECT x.|"), ["column:id", "column:amount"]);
});

test("cheap scope cases: joins in parentheses, LATERAL, USING, words after a table, output names", () => {
  assert.deepEqual(at("SELECT * FROM (orders o JOIN customers c ON o.customer_id = c.id) WHERE c.|"), [
    "column:id",
    "column:name",
    "column:Full Name",
  ]);
  // A LATERAL subquery sees the tables before it.
  assert.ok(columns("SELECT * FROM orders o, LATERAL (SELECT * FROM products WHERE pr| = o.total) p").includes("column:total@o"));
  assert.ok(columns("DELETE FROM orders USING customers WHERE na|").includes("column:name@customers"));
  assert.deepEqual(columns("SELECT to| FROM orders PREWHERE id = 1").at(0), "column:id@orders");
  assert.deepEqual(at("WITH x AS (SELECT 1 n, total::text, a.b::int FROM orders a) SELECT x.| FROM x"), [
    "column:n",
    "column:total",
    "column:b",
  ]);
});

test("an INSERT's target is out of scope in the query giving its rows", () => {
  const inserted = (sql, explicit = false) =>
    (nameOptions(sql.replace("|", ""), sql.indexOf("|"), catalog, explicit)?.items ?? [])
      .filter((i) => i.kind === "column")
      .map((i) => `${i.qualifier ? `${i.qualifier}.` : ""}${i.label}@${i.source}`);
  const orders = ["id@orders", "customer_id@orders", "total@orders", "placed_at@orders"];
  assert.deepEqual(inserted("INSERT INTO customers SELECT * FROM orders WHERE pl|"), orders);
  assert.deepEqual(inserted("INSERT INTO customers (id, name) SELECT | FROM orders", true), orders);
  assert.deepEqual(inserted("INSERT OR REPLACE INTO customers SELECT * FROM orders WHERE i|"), orders);
  assert.deepEqual(inserted("INSERT INTO customers VALUES (|", true), []);
  // Its column list, ON CONFLICT and RETURNING name its columns.
  assert.ok(inserted("INSERT INTO customers (|) SELECT * FROM orders", true).includes("name@customers"));
  assert.ok(inserted("INSERT INTO customers SELECT * FROM orders ON CONFLICT (|", true).includes("name@customers"));
  assert.ok(inserted("INSERT INTO customers SELECT * FROM orders RETURNING na|").includes("name@customers"));
  assert.ok(inserted("INSERT INTO customers VALUES (1, 'a', 'b') ON DUPLICATE KEY UPDATE name = na|").includes("name@customers"));
  // A MERGE target stays in scope.
  assert.ok(inserted("MERGE INTO orders o USING customers s ON s.id = o.customer_id WHEN MATCHED THEN UPDATE SET total = to|").includes("total@o"));
});

test("each dialect's comments and strings are masked as lexer.rs reads them", () => {
  const my = scopeOptions("mysql");
  const pg = scopeOptions("postgres");
  const blank = (n) => " ".repeat(n);
  // `#` comments in MySQL and BigQuery; in ClickHouse before a space or `!`; a name elsewhere.
  assert.equal(maskSql("a # don't\nb", my), `a${blank(8)}\nb`);
  assert.equal(maskSql("a # don't\nb", scopeOptions("bigquery")), `a${blank(8)}\nb`);
  assert.equal(maskSql("a #! x\nb #c", scopeOptions("clickhouse")), `a${blank(5)}\nb #c`);
  assert.equal(maskSql("#tmp 'x'", scopeOptions("sqlserver")), `#tmp${blank(4)}`);
  // MySQL needs a space after `--`.
  assert.equal(maskSql("1--2 -- c", my), `1--2${blank(5)}`);
  // `$tag$` strings in PostgreSQL, not right after a name, and not in MySQL (`DELIMITER $$`).
  assert.equal(maskSql("$f$ a;b $f$ c", pg), `${blank(12)}c`);
  assert.equal(maskSql("a$b$ c", pg), "a$b$ c");
  assert.equal(maskSql("DELIMITER $$\nSELECT 1$$", my), "DELIMITER $$\nSELECT 1$$");
  // Oracle `q'[…]'` strings, with a quote inside; after a name, `q` is part of it.
  const ora = scopeOptions("oracle");
  assert.equal(maskSql("q'[it's]' x", ora), `${blank(10)}x`);
  assert.equal(maskSql("nq'{a}' x", ora), `${blank(8)}x`);
  assert.equal(maskSql("aq'x' y", ora), `aq${blank(4)}y`);
  // The names after such text complete again.
  assert.ok(labels(optionsAt("SELECT 1 # don't\nFROM orders WHERE to|", my)).includes("column:total"));
  const delimiter = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT to| FROM orders; END$$";
  assert.ok(labels(optionsAt(delimiter, my)).includes("column:total"));
});
