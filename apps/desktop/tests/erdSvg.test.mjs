// Unit tests of the SVG an agent gets for an ER diagram (src/erd/diagramSvg.ts): the layout is
// the tab's, and every name is untrusted text that must come out escaped.
import assert from "node:assert/strict";
import { test } from "node:test";
import { erdSvg, escapeXml } from "../src/erd/diagramSvg.ts";

const colors = { series: ["#0969da", "#1a7f37", "#bf8700", "#cf222e", "#8250df", "#0550ae", "#953800", "#9a6700"], text: "#111", muted: "#666", grid: "#ccc", background: "#fff", font: "Inter, sans-serif" };
const column = (name, extra = {}) => ({ name, data_type: "text", nullable: true, primary_key: false, ...extra });
const table = (name, columns, foreign_keys = []) => ({ name, columns, foreign_keys });
const key = (columns, ref_table, ref_columns = ["id"]) => ({ name: `fk_${ref_table}`, columns, ref_table, ref_columns });

const hostile = [
  table("<script>alert(1)</script>", [column("id", { primary_key: true, data_type: "int" }), column('a"&b', { nullable: false }), column("x", { data_type: "<img src=x onerror=alert(1)>" })], [key(["x"], "customer")]),
  table("customer", [column("id", { primary_key: true, data_type: "int" })]),
];

test("names with markup come out as text, never as elements or attributes", () => {
  const svg = erdSvg(hostile, "main", colors);
  assert.ok(!svg.includes("<script"), svg);
  assert.ok(!svg.includes("<img"), svg);
  assert.ok(svg.includes("&lt;script&gt;alert(1)&lt;/script&gt;"), svg);
  assert.ok(svg.includes("a&quot;&amp;b"), svg);
  // A type is cut at 24 characters, and what is left is escaped.
  assert.ok(svg.includes("&lt;img src=x onerror=aler…"), svg);
  // No event attribute and no script element, whatever the names say.
  const markup = svg.replace(/>[^<]*</g, "><");
  assert.ok(!/\son\w+\s*=/i.test(markup), markup);
  assert.ok(!/<\s*script/i.test(svg));
});

test("a quote in a name cannot end an attribute", () => {
  const svg = erdSvg([table('x" onload="alert(1)', [column("id", { primary_key: true })])], "main", { ...colors, font: 'a" onload="b' });
  assert.ok(!svg.includes('" onload='), svg);
  assert.ok(svg.includes("&quot; onload=&quot;"), svg);
});

test("the theme's colours cannot carry markup either", () => {
  const svg = erdSvg(hostile, "main", { ...colors, text: '"/><script>alert(1)</script>', background: "red; x" });
  assert.ok(!svg.includes("<script"), svg);
  assert.ok(svg.includes('fill="#ffffff"'), "a background that is not a colour falls back");
});

test("characters XML cannot hold are replaced", () => {
  assert.equal(escapeXml("a\u0000b\u0008c\u000bd"), "a�b�c�d");
  assert.equal(escapeXml("\uD800x"), "�x");
  assert.equal(escapeXml("ok 😀 \t\n"), "ok 😀 \t\n");
  const svg = erdSvg([table("t\u0000", [column("c\u0001")])], "main", colors);
  assert.ok(!/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/.test(svg));
});

test("the tables, their keys and the line between them are drawn", () => {
  const svg = erdSvg(hostile, "main", colors);
  assert.ok(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg"'), svg);
  assert.ok(svg.endsWith("</svg>"));
  assert.equal((svg.match(/<g class="erd-table"/g) ?? []).length, 2, "one group per table");
  assert.ok(svg.includes(">PK<") && svg.includes(">FK<"));
  assert.equal((svg.match(/marker-start/g) ?? []).length, 1, "one key, one line");
  // A not-null column is starred.
  assert.ok(svg.includes("a&quot;&amp;b *"), svg);
});

test("a key to a table that is not drawn, or to another schema, has no line", () => {
  const lone = erdSvg([table("a", [column("id", { primary_key: true }), column("b_id")], [key(["b_id"], "b")])], "main", colors);
  assert.ok(!lone.includes("marker-start"));
  const other = erdSvg(
    [table("a", [column("id", { primary_key: true }), column("b_id")], [{ ...key(["b_id"], "b"), ref_schema: "other" }]), table("b", [column("id")])],
    "main",
    colors,
  );
  assert.ok(!other.includes("marker-start"));
});

test("an empty model is a blank image of the minimum size", () => {
  const svg = erdSvg([], "main", colors);
  assert.ok(svg.includes('width="400" height="300"'), svg);
  assert.ok(svg.endsWith("</svg>"));
});

test("a very long name is cut to the box", () => {
  const svg = erdSvg([table("n".repeat(500), [column("c".repeat(500))])], "main", colors);
  assert.ok(!svg.includes("n".repeat(100)), "the head is cut");
  assert.ok(svg.includes("…"));
});
