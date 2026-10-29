// Unit tests of the computed columns' expressions (src/grid/computed.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { compile, compileAll, ExpressionError } from "../src/grid/computed.ts";

const col = (name) => ({ name, db_type: "text", numeric: false, binary: false });
const columns = [col("price"), col("qty"), col("first_name"), col("Last Name"), col("note"), col("ID")];

/** Evaluates `expr` on one row, given as the cells' text in column order. */
const run = (expr, row) => compile(expr, columns)((c) => row[c]);
const row = ["12.50", "3", "Ada", "Lovelace", null, "7"];

test("arithmetic on the text of numeric cells", () => {
  assert.equal(run("price * qty", row), "37.5");
  assert.equal(run("price * qty - 0.5", row), "37");
  assert.equal(run("-qty + 10 % 4", row), "-1");
  assert.equal(run("(price + 0.1) * 2", row), "25.2");
  assert.equal(run("0.1 + 0.2", row), "0.3");
  assert.equal(run("qty / 2", row), "1.5");
});

test("NULL spreads through operations, as in SQL", () => {
  assert.equal(run("note || 'x'", row), null);
  assert.equal(run("qty + note", row), null);
  assert.equal(run("coalesce(note, 'none')", row), "none");
  assert.equal(run("note IS NULL", row), "true");
  assert.equal(run("qty IS NOT NULL", row), "true");
});

test("text that is not a number gives NULL in arithmetic, and division by zero too", () => {
  assert.equal(run("first_name + 1", row), null);
  assert.equal(run("qty / 0", row), null);
  assert.equal(run("qty % 0", row), null);
});

test("joining text and the text functions", () => {
  assert.equal(run("first_name || ' ' || \"Last Name\"", row), "Ada Lovelace");
  assert.equal(run("upper(first_name)", row), "ADA");
  assert.equal(run("lower([Last Name])", row), "lovelace");
  assert.equal(run("length(`Last Name`)", row), "8");
  assert.equal(run("substr('Lovelace', 1, 4)", row), "Love");
  assert.equal(run("substr('Lovelace', 5)", row), "lace");
  assert.equal(run("replace('a-b-c', '-', '/')", row), "a/b/c");
  assert.equal(run("concat(first_name, note, '!')", row), "Ada!");
  assert.equal(run("trim('  x ')", row), "x");
  assert.equal(run("'it''s'", row), "it's");
});

test("numeric functions", () => {
  assert.equal(run("round(price / 7, 2)", row), "1.79");
  assert.equal(run("round(price)", row), "13");
  assert.equal(run("abs(-qty)", row), "3");
  assert.equal(run("floor(price)", row), "12");
  assert.equal(run("ceil(price)", row), "13");
});

test("comparisons and logic", () => {
  assert.equal(run("if(qty >= 3, 'bulk', 'single')", row), "bulk");
  assert.equal(run("if(qty > 3 OR price > 10, 'yes', 'no')", row), "yes");
  assert.equal(run("NOT (qty = 3) AND TRUE", row), "false");
  assert.equal(run("price > '9'", row), "true");
  assert.equal(run("first_name = 'Ada'", row), "true");
  assert.equal(run("nullif(qty, 3)", row), null);
  assert.equal(run("first_name <> 'Bob'", row), "true");
});

test("names match exactly, then ignoring case when only one column fits", () => {
  assert.equal(run("id * 2", row), "14");
  assert.equal(run("QTY", row), "3");
  // Quoted names match exactly.
  assert.equal(run('"qty"', row), "3");
  assert.throws(() => compile('"QTY"', columns), (e) => e instanceof ExpressionError && e.reason === "column");
});

test("a cell not loaded yet leaves the value unknown", () => {
  assert.equal(compile("price * qty", columns)((c) => (c === 1 ? undefined : row[c])), undefined);
  // COALESCE stops at the first value that is not NULL, before the unloaded one.
  assert.equal(compile("coalesce(first_name, qty)", columns)((c) => (c === 1 ? undefined : row[c])), "Ada");
});

test("errors name what is wrong", () => {
  const reason = (expr) => {
    try {
      compile(expr, columns);
      return null;
    } catch (e) {
      return e instanceof ExpressionError ? [e.reason, e.near] : String(e);
    }
  };
  assert.deepEqual(reason("missing + 1"), ["column", "missing"]);
  assert.deepEqual(reason("nope(qty)"), ["function", "nope"]);
  assert.deepEqual(reason("round()"), ["arguments", "round"]);
  assert.equal(reason("price *")?.[0], "syntax");
  assert.equal(reason("(price")?.[0], "syntax");
  assert.equal(reason("'open")?.[0], "syntax");
  assert.equal(reason("price ? 2")?.[0], "syntax");
  assert.equal(reason("price qty")?.[0], "syntax");
});

test("compileAll keeps the columns that compile and the error of the others", () => {
  const out = compileAll(
    [
      { name: "total", expression: "price * qty" },
      { name: "bad", expression: "nope" },
    ],
    columns,
  );
  assert.equal(out[0].error, null);
  assert.equal(out[0].evaluate((c) => row[c]), "37.5");
  assert.equal(out[1].evaluate, null);
  assert.equal(out[1].error.reason, "column");
});
