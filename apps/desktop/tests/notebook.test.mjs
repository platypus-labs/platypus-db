// Unit tests of the notebook: cell names and the results a cell reads, the file format, the
// plan as lines (src/notebook/model.ts), Markdown and the HTML export (src/notebook/html.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { markdownHtml, notebookHtml, parseInline, parseMarkdown } from "../src/notebook/html.ts";
import {
  cellNames,
  missingParams,
  newCell,
  notebookFile,
  parseNotebookFile,
  planLines,
  readContent,
  resultTables,
} from "../src/notebook/model.ts";

const keep = (v) => v ?? null;

test("SQL cells are c1, c2… in their order; text cells have no name", () => {
  const cells = [newCell("markdown", "# Intro"), newCell("sql", "SELECT 1"), newCell("markdown"), newCell("sql", "SELECT 2")];
  const names = cellNames(cells);
  assert.deepEqual([...names.values()], ["c1", "c2"]);
  assert.equal(names.get(cells[3].id), "c2");
  assert.equal(names.has(cells[0].id), false);
});

test("a cell on the results reads the other cells that have rows, by name", () => {
  const cells = [newCell("sql", "a"), newCell("sql", "b"), newCell("sql", "c")];
  const jobs = new Map([
    [cells[0].id, 11],
    [cells[2].id, 33],
  ]);
  assert.deepEqual(resultTables(cells, jobs, cells[2].id), [{ job_id: 11, table: "c1" }]);
  assert.deepEqual(resultTables(cells, jobs, cells[1].id), [
    { job_id: 11, table: "c1" },
    { job_id: 33, table: "c3" },
  ]);
});

test("a notebook file goes and comes back; another file is refused", () => {
  const cells = [
    { ...newCell("sql", "SELECT :n"), onResults: false, chart: { shown: true, config: { kind: "bar", x: "a", y: ["b"], aggregate: "sum" } }, plan: true },
    newCell("markdown", "*hi*"),
  ];
  const text = notebookFile({ cells, params: { n: "3" } });
  assert.ok(!text.includes(cells[0].id), "ids are not saved");
  const back = parseNotebookFile(text, keep);
  assert.deepEqual(
    back.cells.map(({ id, ...c }) => c),
    cells.map(({ id, ...c }) => c),
  );
  assert.deepEqual(back.params, { n: "3" });
  assert.throws(() => parseNotebookFile("{}", keep), /not a notebook/);
  assert.throws(() => parseNotebookFile("SELECT 1", keep), /not a notebook/);
});

test("what a tab saved is read whatever it holds wrongly", () => {
  const content = readContent(
    {
      cells: [{ kind: "sql", text: "x", onResults: "yes", chart: "no" }, { kind: "python", text: "x" }, { kind: "markdown" }, null],
      params: { a: "1", b: 2 },
    },
    keep,
  );
  assert.equal(content.cells.length, 1);
  assert.equal(content.cells[0].onResults, false);
  assert.equal(content.cells[0].chart, null);
  assert.deepEqual(content.params, { a: "1" });
  assert.deepEqual(readContent(null, keep), { cells: [], params: {} });
});

test("parameters without a value", () => {
  assert.deepEqual(missingParams(["a", "b", "c"], { a: "1", b: "" }), ["b", "c"]);
});

test("a plan becomes indented lines", () => {
  const node = (operation, children = []) => ({ operation, detail: null, cost: 1, rows: 2, actual_rows: null, actual_ms: null, loops: null, self_cost: null, self_ms: null, full_scan: null, properties: [], children });
  const lines = planLines(node("Hash Join", [node("Seq Scan"), node("Hash", [node("Index Scan")])]));
  assert.deepEqual(
    lines.map((l) => [l.depth, l.operation]),
    [
      [0, "Hash Join"],
      [1, "Seq Scan"],
      [1, "Hash"],
      [2, "Index Scan"],
    ],
  );
});

test("Markdown blocks", () => {
  const blocks = parseMarkdown("# Title\n\nSome *text*\nmore\n\n- one\n- two\n  continued\n\n1. first\n\n> quoted\n\n```sql\nSELECT 1;\n```\n\n---");
  assert.deepEqual(
    blocks.map((b) => b.kind),
    ["heading", "paragraph", "list", "list", "quote", "code", "rule"],
  );
  assert.equal(blocks[2].items.length, 2);
  assert.deepEqual(blocks[2].items[1], [{ kind: "text", text: "two continued" }]);
  assert.equal(blocks[3].ordered, true);
  assert.equal(blocks[5].text, "SELECT 1;");
});

test("Markdown inline: code, bold, italic, links; snake_case stays text", () => {
  assert.deepEqual(parseInline("a `x*y` **b** _c_ snake_case_name"), [
    { kind: "text", text: "a " },
    { kind: "code", text: "x*y" },
    { kind: "text", text: " " },
    { kind: "strong", children: [{ kind: "text", text: "b" }] },
    { kind: "text", text: " " },
    { kind: "em", children: [{ kind: "text", text: "c" }] },
    { kind: "text", text: " snake_case_name" },
  ]);
  assert.deepEqual(parseInline("[docs](https://example.com)"), [
    { kind: "link", href: "https://example.com", children: [{ kind: "text", text: "docs" }] },
  ]);
  // A link to anything but the web or mail is its text.
  assert.deepEqual(parseInline("[x](javascript:alert(1))"), [{ kind: "text", text: "x" }, { kind: "text", text: ")" }]);
});

test("no text becomes markup in the HTML", () => {
  const html = markdownHtml('# <b>"Hi"</b>\n\n<script>alert(1)</script> & [a](https://x.y/?a=1&b="2")');
  assert.ok(!html.includes("<script>"), html);
  assert.ok(html.includes("<h1>&lt;b&gt;&quot;Hi&quot;&lt;/b&gt;</h1>"), html);
  assert.ok(html.includes('href="https://x.y/?a=1&amp;b=&quot;2&quot;"'), html);
});

test("the export is a page with every cell", () => {
  const page = notebookHtml("Sales <2026>", [
    { kind: "markdown", text: "## Orders" },
    {
      kind: "sql",
      name: "c1",
      sql: "SELECT id, note FROM t",
      table: { columns: ["id", "note"], rows: [["1", "<b>"], ["2", null]] },
      summary: "2 rows",
      failed: false,
      chart: null,
      plan: [{ depth: 0, operation: "Seq Scan", detail: "on t", rows: 2, cost: 1.5 }],
    },
    { kind: "sql", name: "c2", sql: "SELECT nope", table: null, summary: "no such column", failed: true, chart: null, plan: null },
  ], { min: "<5>" });
  assert.ok(page.includes("<code>:min</code> = &lt;5&gt;"), "the parameters' values");
  assert.ok(page.startsWith("<!doctype html>"));
  assert.ok(page.includes("<title>Sales &lt;2026&gt;</title>"));
  assert.ok(page.includes("<h2>Orders</h2>"));
  assert.ok(page.includes("<td>&lt;b&gt;</td>") && page.includes('<td class="null">NULL</td>'));
  assert.ok(page.includes("Seq Scan on t  (rows=2 cost=1.5)"));
  assert.ok(page.includes('class="summary failed">no such column'));
});
