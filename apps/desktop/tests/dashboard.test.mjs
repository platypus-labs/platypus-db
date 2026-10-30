// Unit tests of the dashboard model (src/dashboard/model.ts): widgets and their limits, the
// connection a widget runs on, the read-only check of a script, and the file format.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CHART_ROWS,
  connectionOf,
  dashboardFile,
  DEFAULT_ROWS,
  findScript,
  moveWidget,
  newWidget,
  parseDashboardFile,
  readContent,
  refusal,
  rowLimit,
  valueText,
  widgetTitle,
} from "../src/dashboard/model.ts";

const chart = (v) => (v && v.kind === "bar" ? v : null);
const query = { read_only: true, kind: "query", parameters: [] };

test("a widget reads as many rows as its view can show", () => {
  assert.equal(rowLimit({ view: "value", rows: 100 }), 1);
  assert.equal(rowLimit({ view: "chart", rows: 5 }), CHART_ROWS);
  assert.equal(rowLimit({ view: "table", rows: 25 }), 25);
  assert.equal(rowLimit({ view: "table", rows: 0 }), DEFAULT_ROWS, "no number: the default");
  assert.equal(rowLimit({ view: "table", rows: 10 ** 9 }), 10_000);
  assert.equal(rowLimit({ view: "table", rows: -3 }), 1);
});

test("a widget is called by its title, else by its script", () => {
  assert.equal(widgetTitle({ title: " Totals ", script: "a.sql" }), "Totals");
  assert.equal(widgetTitle({ title: "", script: "reports/Monthly sales.sql" }), "Monthly sales");
});

test("a widget runs on its own connection, else its script's, else the dashboard's", () => {
  assert.equal(connectionOf({ connection: "w" }, { connection: "s" }, "d"), "w");
  assert.equal(connectionOf({ connection: null }, { connection: "s" }, "d"), "s");
  assert.equal(connectionOf({ connection: null }, { connection: null }, "d"), "d");
  assert.equal(connectionOf({ connection: null }, null, "d"), "d");
});

test("a script is found by its path, and a folder is no script", () => {
  const entries = [
    { path: "reports", folder: true, connection: null },
    { path: "reports/a.sql", folder: false, connection: null },
  ];
  assert.equal(findScript(entries, "reports/a.sql")?.path, "reports/a.sql");
  assert.equal(findScript(entries, "reports"), undefined);
  assert.equal(findScript(entries, "gone.sql"), undefined, "renamed or deleted");
});

test("only scripts that read, without commands or parameters, feed a widget", () => {
  assert.equal(refusal([query]), null);
  assert.equal(refusal([query, query]), null);
  assert.equal(refusal([]), "empty");
  assert.equal(refusal([query, { ...query, read_only: false, kind: "dml" }]), "changes");
  assert.equal(refusal([{ ...query, read_only: false, kind: "ddl" }]), "changes");
  assert.equal(refusal([{ ...query, kind: "command" }]), "command");
  assert.equal(refusal([{ ...query, parameters: [{ kind: "named" }] }]), "parameters");
});

test("moving a widget swaps it with its neighbour, and stops at the ends", () => {
  const [a, b, c] = ["a", "b", "c"].map((id) => newWidget({ id, script: `${id}.sql` }));
  const ids = (widgets) => widgets.map((w) => w.id).join("");
  assert.equal(ids(moveWidget([a, b, c], "b", -1)), "bac");
  assert.equal(ids(moveWidget([a, b, c], "b", 1)), "acb");
  assert.equal(ids(moveWidget([a, b, c], "a", -1)), "abc");
  assert.equal(ids(moveWidget([a, b, c], "c", 1)), "abc");
  assert.equal(ids(moveWidget([a, b, c], "x", 1)), "abc");
});

test("SQL NULL is written as NULL in a single value", () => {
  assert.equal(valueText(null), "NULL");
  assert.equal(valueText("42"), "42");
  assert.equal(valueText(""), "");
});

test("a dashboard file keeps the widgets and reads back the same", () => {
  const widgets = [
    newWidget({ id: "w1", title: "Rows", script: "a.sql", view: "table", rows: 20, width: 2 }),
    newWidget({ id: "w2", script: "b.sql", view: "chart", chart: { kind: "bar", x: "region", y: ["total"], aggregate: "sum" }, height: 2, connection: "c1" }),
    newWidget({ id: "w3", script: "c.sql", view: "value" }),
  ];
  const text = dashboardFile("Sales", { widgets, refresh: 5, connection: "c0" });
  const read = parseDashboardFile(text, chart);
  assert.equal(read.title, "Sales");
  assert.deepEqual(read.content, { widgets, refresh: 5, connection: "c0" });
});

test("a file of another kind is refused", () => {
  assert.throws(() => parseDashboardFile("not json", chart), /not a dashboard file/);
  assert.throws(() => parseDashboardFile("{}", chart), /not a dashboard file/);
  assert.throws(() => parseDashboardFile(JSON.stringify({ format: "platypus-notebook" }), chart), /not a dashboard file/);
});

test("what a damaged save lacks or holds wrongly is dropped or defaulted", () => {
  const content = readContent({
    refresh: 7,
    widgets: [
      null,
      "x",
      { script: "" },
      { title: 3 },
      { script: "a.sql", view: "gauge", rows: "many", width: 9, height: 0, chart: { kind: "donut" }, connection: 5 },
      { id: "keep", script: "b.sql", view: "chart", rows: 2.6, width: 2.4, height: 2 },
    ],
  }, chart);
  assert.equal(content.refresh, 0, "an interval the dashboard does not offer is off");
  assert.equal(content.widgets.length, 2);
  const [first, second] = content.widgets;
  assert.deepEqual(
    { title: first.title, view: first.view, rows: first.rows, width: first.width, height: first.height, chart: first.chart, connection: first.connection },
    { title: "", view: "table", rows: DEFAULT_ROWS, width: 3, height: 1, chart: null, connection: null },
  );
  assert.ok(first.id, "a widget without id gets one");
  assert.deepEqual({ id: second.id, view: second.view, rows: second.rows, width: second.width }, { id: "keep", view: "chart", rows: 3, width: 2 });
  assert.deepEqual(readContent(undefined, chart), { widgets: [], refresh: 0, connection: null });
  assert.deepEqual(readContent("text", chart), { widgets: [], refresh: 0, connection: null });
});

test("new widgets have different ids", () => {
  assert.notEqual(newWidget().id, newWidget().id);
});
