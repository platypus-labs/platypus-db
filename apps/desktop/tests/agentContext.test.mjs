// Unit tests of what agents may see of the user's work (src/agents/context.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  AGENT_TEXT_BYTES,
  RESULT_ROWS,
  buildContext,
  followActiveTab,
  grant,
  granted,
  noGrants,
  provideContext,
  readActive,
  resultRows,
  selectionText,
  shareContext,
  summarize,
  verdictFor,
  viewerTable,
} from "../src/agents/context.ts";
import { enqueue, settle } from "../src/agents/writes.ts";

const snapshot = (over = {}) => ({
  connection: "Shop",
  connectionId: "00000000-0000-4000-8000-000000000001",
  title: "orders.sql",
  editor: "SELECT * FROM orders",
  selection: "orders",
  result: { columns: ["id", "note"], rows: [["1", null]], truncated: false },
  ...over,
});

test("the selection is cut from the text, whichever way it was made", () => {
  assert.equal(selectionText("SELECT 1", 7, 8), "1");
  assert.equal(selectionText("SELECT 1", 8, 7), "1");
  assert.equal(selectionText("SELECT 1", 3, 3), "");
  assert.equal(selectionText("SELECT 1", -4, 6), "SELECT");
});

test("only the parts asked for are built", () => {
  const all = buildContext(["editor", "selection", "result"], snapshot());
  assert.equal(all.connection, "Shop");
  assert.equal(all.connection_id, "00000000-0000-4000-8000-000000000001");
  assert.equal(all.tab, "orders.sql");
  assert.equal(all.editor, "SELECT * FROM orders");
  assert.equal(all.selection, "orders");
  assert.deepEqual(all.result.rows, [["1", null]]);

  const some = buildContext(["selection"], snapshot());
  assert.equal(some.editor, null);
  assert.equal(some.result, null);
  assert.equal(some.selection, "orders");
  assert.equal(some.connection, "Shop", "the agent is told whose tab it is");

  assert.equal(buildContext(["selection"], snapshot({ selection: "" })).selection, null, "nothing selected");
});

test("a data viewer tab names its table, whatever its filter; a free query names none", () => {
  const ref = { schema: "main", name: "people", kind: "table" };
  assert.deepEqual(viewerTable(true, ref), { schema: "main", name: "people" });
  assert.equal(viewerTable(false, ref), undefined, "an editor tab with a free query");
  assert.equal(viewerTable(true, null), undefined);
  const viewer = buildContext(["result"], snapshot({ table: viewerTable(true, ref) }));
  assert.deepEqual(viewer.table, { schema: "main", name: "people" });
  assert.equal("table" in buildContext(["result"], snapshot()), false);
  assert.equal("table" in buildContext(["result"], null), false);
});

test("without a query tab everything is empty", () => {
  assert.deepEqual(buildContext(["editor", "selection", "result"], null), {
    connection: null,
    connection_id: null,
    tab: null,
    editor: null,
    selection: null,
    result: null,
  });
  const summary = summarize(["editor", "result"], null);
  assert.equal(summary.tab, false);
  assert.deepEqual(
    summary.items.map((i) => i.present),
    [false, false],
  );
});

test("a result keeps its first rows and says when there are more", () => {
  const names = ["a", "b"];
  const columns = [
    Array.from({ length: 150 }, (_, i) => String(i)),
    Array.from({ length: 150 }, () => null),
  ];
  const long = resultRows(names, columns, 150, false);
  assert.equal(long.rows.length, RESULT_ROWS);
  assert.deepEqual(long.rows[1], ["1", null]);
  assert.equal(long.truncated, true);
  assert.equal(long.total_rows, 150, "the real size travels with the cut rows");
  const short = resultRows(names, [["x"], ["y"]], 1, false);
  assert.deepEqual(short.rows, [["x", "y"]]);
  assert.equal(short.truncated, false);
  assert.equal(resultRows(names, [["x"], ["y"]], 1, true).truncated, true, "the server has more");
  assert.equal("total_rows" in resultRows(names, [["x"], ["y"]], 1, true), false, "the size is not known while fetching");
  assert.deepEqual(resultRows([], [], 0, false).rows, []);
});

test("the dialog lists what would be shared, with its sizes", () => {
  const summary = summarize(["editor", "selection", "result"], snapshot());
  assert.equal(summary.tab, true);
  assert.equal(summary.title, "orders.sql");
  assert.equal(summary.connection, "Shop");
  assert.deepEqual(
    summary.items.map(({ part, present, chars, rows, columns, cut }) => [part, present, chars, rows, columns, cut]),
    [
      ["editor", true, 20, 0, 0, false],
      ["selection", true, 6, 0, 0, false],
      ["result", true, 0, 1, 2, false],
    ],
  );
  // Only what was asked for is listed, once each.
  assert.deepEqual(
    summarize(["result", "result"], snapshot({ result: null })).items.map((i) => [i.part, i.present]),
    [["result", false]],
  );
  // The server gives the agent 64 KiB of text: the dialog says so for more.
  const big = summarize(["editor"], snapshot({ editor: "é".repeat(AGENT_TEXT_BYTES / 2 + 1) }));
  assert.equal(big.items[0].cut, true);
  assert.equal(summarize(["editor"], snapshot({ editor: "é".repeat(AGENT_TEXT_BYTES / 2) })).items[0].cut, false);
});

test("the dialog tells the real size of the result, not the rows cut to 100", () => {
  const rows = Array.from({ length: RESULT_ROWS }, (_, i) => [String(i)]);
  const cut = { columns: ["n"], rows, truncated: true, total_rows: 150 };
  const known = summarize(["result"], snapshot({ result: cut })).items[0];
  assert.deepEqual([known.rows, known.exact, known.cut], [150, true, true]);
  // Still fetching: only a lower bound is honest.
  const fetching = summarize(["result"], snapshot({ result: { columns: ["n"], rows, truncated: true } })).items[0];
  assert.deepEqual([fetching.rows, fetching.exact, fetching.cut], [100, false, true]);
  const small = summarize(["result"], snapshot()).items[0];
  assert.deepEqual([small.rows, small.exact, small.cut], [1, true, false]);
});

test("a tab on a connection closed to agents shares nothing and is refused", () => {
  const closed = snapshot({ closedToAgents: true });
  const context = buildContext(["editor", "selection", "result"], closed);
  assert.deepEqual([context.editor, context.selection, context.result], [null, null, null]);
  assert.equal(context.connection_id, "00000000-0000-4000-8000-000000000001");
  const summary = summarize(["editor", "result"], closed);
  assert.equal(summary.closed, true);
  assert.deepEqual(
    summary.items.map((i) => i.present),
    [false, false],
  );
  assert.equal(summarize(["editor"], snapshot()).closed, false);
  // Even a session already allowed is refused, not answered with an empty context.
  for (const how of ["once", "session"]) {
    const verdict = verdictFor(["editor"], closed, how);
    assert.equal(verdict.answer, "deny");
    assert.match(verdict.reason, /closed to agents/);
  }
});

test("the answer carries the snapshot it is given, not a new reading", async () => {
  followActiveTab(9);
  const off = provideContext({ tabKey: 9, snapshot: async () => snapshot({ editor: "SELECT 'changed'" }) });
  const shown = snapshot({ editor: "SELECT 'shown'" });
  const verdict = verdictFor(["editor"], shown, "once");
  assert.equal(verdict.answer, "once");
  assert.equal(verdict.context.editor, "SELECT 'shown'");
  assert.equal(verdictFor(["editor"], null, "session").context.editor, null, "no SQL tab: empty context");
  off();
});

test("the active tab is the one read, as it is at that moment", async () => {
  let text = "SELECT 1";
  const host = (tabKey) => ({ tabKey, snapshot: async () => snapshot({ editor: `${text} -- ${tabKey}` }) });
  const off = [provideContext(host(1)), provideContext(host(2))];
  followActiveTab(null);
  assert.equal(await readActive(), null);
  followActiveTab(2);
  assert.equal((await shareContext(["editor"])).editor, "SELECT 1 -- 2");
  text = "SELECT 2";
  assert.equal((await shareContext(["editor"])).editor, "SELECT 2 -- 2", "read again each time");
  followActiveTab(7);
  assert.equal(await readActive(), null, "a tab that is not an SQL editor");
  off.forEach((f) => f());
  followActiveTab(2);
  assert.equal(await readActive(), null, "closed tabs are not read");
});

test("allowing a session holds for it alone", () => {
  assert.equal(granted(noGrants, "s1"), false);
  const one = grant(noGrants, "s1");
  assert.equal(granted(one, "s1"), true);
  assert.equal(granted(one, "s2"), false);
  assert.equal(granted(noGrants, "s1"), false, "the earlier grants do not change");
  assert.equal(granted(grant(one, "s2"), "s1"), true);
});

test("requests to see the user's work queue like changes do", () => {
  const request = (id) => ({ id, agent: "claude-code", session: "s1", parts: ["editor"] });
  let queue = enqueue([], request("a"), 10);
  queue = enqueue(queue, request("b"), 20);
  queue = enqueue(queue, request("a"), 30);
  assert.deepEqual(
    queue.map((w) => [w.request.id, w.since]),
    [
      ["a", 10],
      ["b", 20],
    ],
  );
  assert.deepEqual(
    settle(queue, "a").map((w) => w.request.id),
    ["b"],
  );
});
