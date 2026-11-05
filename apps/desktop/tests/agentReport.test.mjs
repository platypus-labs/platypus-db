// Unit tests of the agent report exports (src/agents/agentReport.ts): Markdown and HTML built
// from the summary, with hostile agent names, table names and error text.
import assert from "node:assert/strict";
import { test } from "node:test";
import { MISSING, hasOlderCalls, htmlEscape, markdownCell, periodText, reportHtml, reportMarkdown } from "../src/agents/agentReport.ts";

const HOSTILE = "<img src=x onerror=alert(1)>";
const PIPED = ['"q"|`c` x', "second line"].join(String.fromCharCode(10));

const text = {
  title: "Agent report",
  subtitle: "Made 2026-11-05 10:00Z · 4 calls",
  agent: "Agent",
  period: "Period",
  calls: "Calls",
  tables: "Tables read",
  rows: "Rows returned",
  allowed: "Changes allowed",
  refused: "Changes refused",
  errors: "Errors",
  partial: "Older calls show –.",
  estimate: "Tables are an estimate.",
};

const T0 = Date.UTC(2026, 10, 5, 10, 0);
const figures = (over = {}) => ({
  first_at: T0,
  last_at: T0 + 20 * 60_000,
  calls: 4,
  measured: 4,
  tables_read: 2,
  rows_returned: 30,
  changes_allowed: 1,
  changes_refused: 1,
  errors: 1,
  ...over,
});
const report = (over = {}) => [
  { agent: HOSTILE, ...figures(over), sessions: [{ session: PIPED, ...figures(over) }] },
  { agent: "codex", ...figures({ calls: 2, measured: 2 }), sessions: [] },
];

test("the period is in UTC and short within a day", () => {
  assert.equal(periodText(T0, T0), "2026-11-05 10:00Z");
  assert.equal(periodText(T0, T0 + 20 * 60_000), "2026-11-05 10:00Z – 10:20Z");
  assert.equal(periodText(T0, T0 + 86_400_000), "2026-11-05 10:00Z – 2026-11-06 10:00Z");
});

test("a Markdown cell cannot break the table", () => {
  const cell = markdownCell(PIPED);
  assert.doesNotMatch(cell, /\n/);
  assert.doesNotMatch(cell, /(^|[^\\])\|/);
  assert.doesNotMatch(cell, /(^|[^\\])`/);
  assert.doesNotMatch(markdownCell(HOSTILE), /</);
  assert.equal(markdownCell("a\r\nb c"), "a b c");
});

test("the Markdown report is one table with a row per agent and session", () => {
  const md = reportMarkdown(report(), text);
  const lines = md.split("\n");
  assert.equal(lines[0], "# Agent report");
  const table = lines.filter((l) => l.startsWith("|"));
  // Header, rule, two agents and one session.
  assert.equal(table.length, 5);
  // Every row has the same number of unescaped cells.
  const count = (l) => l.replace(/\\\|/g, "").split("|").length;
  assert.deepEqual(new Set(table.map(count)), new Set([count(table[0])]));
  assert.ok(!md.includes(HOSTILE), "the hostile name is escaped");
  assert.ok(md.includes("&lt;img src=x onerror=alert(1)&gt;"));
  assert.ok(md.includes("Tables are an estimate."));
  assert.ok(!md.includes("Older calls"), "nothing partial: no note");
});

test("figures never recorded show a dash, never a zero, and the report says so", () => {
  const old = report({ measured: 0, tables_read: null, rows_returned: null, changes_allowed: null, changes_refused: null });
  assert.equal(hasOlderCalls(old), true);
  const md = reportMarkdown(old, text);
  const row = md.split("\n").find((l) => l.includes("**codex**"));
  assert.ok(row.includes("| 2 |"));
  const hostileRow = md.split("\n").find((l) => l.includes("onerror"));
  assert.equal(hostileRow.split("|").filter((c) => c.trim() === MISSING).length, 4);
  assert.ok(md.includes("Older calls show –."));
  assert.ok(reportHtml(old, text).includes("Older calls show –."));
});

test("an empty report is a header and notes", () => {
  const md = reportMarkdown([], text);
  assert.equal(md.split("\n").filter((l) => l.startsWith("|")).length, 2);
  assert.equal(hasOlderCalls([]), false);
  assert.ok(reportHtml([], text).includes("<tbody>"));
});

test("the HTML report escapes every name and carries no script nor external resource", () => {
  const html = reportHtml(report(), text);
  assert.ok(!html.includes(HOSTILE));
  assert.ok(!/<img/i.test(html));
  assert.ok(html.includes("&#60;img src=x onerror=alert(1)&#62;"));
  assert.ok(!/<script/i.test(html));
  assert.ok(!/<[^>]*onerror/i.test(html), "no tag carries the handler");
  const tags = new Set([...html.matchAll(/<\/?([a-z0-9]+)/gi)].map((m) => m[1].toLowerCase()));
  assert.deepEqual(
    [...tags].sort(),
    ["body", "h1", "head", "html", "meta", "p", "style", "table", "tbody", "td", "th", "thead", "title", "tr"],
  );
  assert.ok(!/url\(|@import/i.test(html));
  assert.ok(html.includes("default-src 'none'"));
  // The quotes of the other name are inert too.
  assert.ok(html.includes("&#34;q&#34;|"));
  assert.ok(html.startsWith("<!doctype html>"));
});

test("a hostile language tag cannot leave the attribute", () => {
  const html = reportHtml(report(), text, 'en"><script>alert(1)</script>');
  assert.ok(!/<script/i.test(html));
});

test("htmlEscape covers the five characters", () => {
  assert.equal(htmlEscape(`<>&"'`), "&#60;&#62;&#38;&#34;&#39;");
});
