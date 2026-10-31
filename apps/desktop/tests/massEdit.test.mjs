// Unit tests of the data editor's mass edits (src/grid/edit/massEdit.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { Worker } from "node:worker_threads";
import {
  checkValue,
  columnRule,
  fillDown,
  loadsFromFile,
  numberSequence,
  isHex,
  replaceInputs,
  replaceText,
  replacedOutcome,
  runReplaceJob,
  replacer,
  selectionCells,
  sequenceValue,
  setValue,
} from "../src/grid/edit/massEdit.ts";
import { runReplace } from "../src/grid/edit/replaceRunner.ts";
import { rebase } from "../src/grid/edit/state.ts";

const info = (db_type, extra = {}) => ({ name: "c", db_type, numeric: false, binary: false, ...extra });
const meta = (data_type, extra = {}) => ({
  name: "c",
  data_type,
  nullable: true,
  default: null,
  primary_key: false,
  comment: null,
  enum_values: [],
  fields: [],
  ...extra,
});
const value = (text) => ({ kind: "value", text });
const NULL = { kind: "null" };
const DEFAULT = { kind: "default" };

/** Columns 0: integer, 1: text, 2: real (float), 3: not the table's. */
const RULES = [
  columnRule(info("INTEGER", { numeric: true }), meta("INTEGER", { nullable: false, primary_key: true })),
  columnRule(info("TEXT"), meta("TEXT")),
  columnRule(info("REAL", { numeric: true }), meta("REAL")),
  undefined,
];
const rules = (col) => RULES[col];
const cell = (gridRow, col, current, ...original) => ({ gridRow, col, current, ...(original.length ? { original: original[0] } : {}) });

test("column rules follow the type", () => {
  assert.equal(columnRule(info("int4", { numeric: true }), undefined).kind, "integer");
  assert.equal(columnRule(info("bigint unsigned", { numeric: true }), undefined).kind, "integer");
  assert.equal(columnRule(info("numeric(10,2)", { numeric: true }), undefined).kind, "number");
  assert.equal(columnRule(info("interval"), undefined).kind, "other");
  assert.equal(columnRule(info("point"), undefined).kind, "other");
  assert.equal(columnRule(info("boolean"), undefined).kind, "bool");
  assert.equal(columnRule(info("tinyint(1)", { numeric: true }), undefined).kind, "integer");
  assert.equal(columnRule(info("bytea", { binary: true }), undefined).kind, "binary");
  assert.equal(columnRule(info("jsonb"), undefined).kind, "json");
  assert.equal(columnRule(info("character varying(20)"), undefined).kind, "text");
  assert.equal(columnRule(info("text[]"), undefined).kind, "other");
  assert.equal(columnRule(info("mood"), meta("mood", { enum_values: ["sad", "ok"] })).kind, "enum");
  assert.equal(columnRule(info("double precision", { numeric: true }), undefined).float, true);
  assert.ok(loadsFromFile(columnRule(info("BLOB", { binary: true }), undefined)));
  assert.ok(loadsFromFile(columnRule(info("TEXT"), undefined)));
  assert.ok(!loadsFromFile(columnRule(info("INTEGER", { numeric: true }), undefined)));
});

test("an empty value is NULL where the column holds no empty text", () => {
  const empty = (db_type, extra) => columnRule(info(db_type, extra), undefined).emptyIsNull;
  for (const type of ["date", "time", "timestamp without time zone", "timestamptz", "datetime2(7)", "Nullable(DateTime64(3))", "interval", "year", "uuid", "inet", "boolean", "jsonb"]) {
    assert.ok(empty(type), type);
  }
  assert.ok(empty("integer", { numeric: true }));
  for (const type of ["text", "character varying(20)", "xml", "bytea", "point", "text[]"]) {
    assert.ok(!empty(type, type === "bytea" ? { binary: true } : {}), type);
  }
  const date = columnRule(info("date"), meta("date"));
  assert.deepEqual(setValue([cell(0, 0, value("2024-01-01"))], () => date, value(" ")).changes[0].value, NULL);
});

test("values are checked as saving checks them", () => {
  const rule = (db_type, extra, m) => columnRule(info(db_type, extra), m);
  const int = rule("integer", { numeric: true }, meta("integer", { nullable: false }));
  assert.equal(checkValue(int, value(" 42 ")), null);
  assert.equal(checkValue(int, value("4.2")), "integer");
  assert.equal(checkValue(int, NULL), "null");
  // NOT NULL without a default: DEFAULT would be NULL, unless the database numbers the key.
  assert.equal(checkValue(int, DEFAULT), "default");
  assert.equal(checkValue(rule("integer", { numeric: true }, meta("integer", { nullable: false, default: "0" })), DEFAULT), null);
  assert.equal(checkValue(rule("integer", { numeric: true }, meta("integer", { nullable: false, primary_key: true })), DEFAULT), null);
  assert.equal(checkValue(rule("integer", { numeric: true }), DEFAULT), null);
  const num = rule("numeric", { numeric: true });
  assert.equal(checkValue(num, value("-1.5e3")), null);
  assert.equal(checkValue(num, value("NaN")), "number");
  assert.equal(checkValue(rule("float8", { numeric: true }), value("NaN")), null);
  assert.equal(checkValue(rule("boolean"), value("Yes")), null);
  assert.equal(checkValue(rule("boolean"), value("maybe")), "bool");
  const bin = rule("bytea", { binary: true });
  assert.equal(checkValue(bin, value("\\xDEAD")), null);
  assert.equal(checkValue(bin, value("0xabc")), "binary");
  assert.equal(checkValue(rule("json"), value('{"a": 1}')), null);
  assert.equal(checkValue(rule("json"), value("{a}")), "json");
  const mood = rule("mood", {}, meta("mood", { enum_values: ["sad", "ok"] }));
  assert.equal(checkValue(mood, value("ok")), null);
  assert.equal(checkValue(mood, value("happy")), "enum");
  assert.equal(checkValue(rule("text"), value("anything")), null);
});

test("selected cells read pending values over loaded ones, without deleted rows", () => {
  const state = {
    inserts: [new Map([[1, value("new")]])],
    updates: new Map([[0, new Map([[1, value("changed")]])]]),
    deletes: new Set([1]),
  };
  const loaded = [
    ["1", "a"],
    ["2", "b"],
    ["3", null],
  ];
  const cells = selectionCells(state, { r0: 0, r1: 3, cols: [0, 1] }, loaded, 0);
  assert.deepEqual(cells, [
    cell(0, 0, DEFAULT),
    cell(0, 1, value("new")),
    cell(1, 0, value("1"), "1"),
    cell(1, 1, value("changed"), "a"),
    // Grid row 2 is result row 1, deleted.
    cell(3, 0, value("3"), "3"),
    cell(3, 1, NULL, null),
  ]);
});

test("a value goes into every cell that can take it; the others are reported", () => {
  const cells = [cell(0, 0, value("1"), "1"), cell(0, 1, value("a"), "a"), cell(1, 0, value("2"), "2"), cell(1, 3, value("x"), "x")];
  const out = setValue(cells, rules, value("abc"));
  assert.deepEqual(out.changes, [{ gridRow: 0, col: 1, value: value("abc"), original: "a" }]);
  assert.deepEqual(out.rejected, [
    { col: 0, problem: "integer", cells: 2 },
    { col: 3, problem: "readonly", cells: 1 },
  ]);
  // An empty value in a number column is NULL, which this one does not take.
  assert.deepEqual(setValue([cell(0, 0, value("1"), "1")], rules, value(" ")).rejected, [{ col: 0, problem: "null", cells: 1 }]);
  assert.deepEqual(setValue([cell(0, 2, value("1"), "1")], rules, value("")).changes[0].value, NULL);
  assert.equal(setValue(cells, rules, DEFAULT).changes.length, 3);
});

test("fill down copies the first selected row of each column", () => {
  const cells = [
    cell(4, 0, value("7"), "7"),
    cell(4, 1, NULL, null),
    cell(5, 0, value("8"), "8"),
    cell(5, 1, value("b"), "b"),
    cell(6, 0, value("9"), "9"),
    cell(6, 1, value("c"), "c"),
  ];
  const out = fillDown(cells, rules);
  assert.deepEqual(
    out.changes.map((c) => [c.gridRow, c.col, c.value]),
    [
      [5, 0, value("7")],
      [5, 1, NULL],
      [6, 0, value("7")],
      [6, 1, NULL],
    ],
  );
  assert.deepEqual(out.rejected, []);
  assert.deepEqual(fillDown([cell(0, 1, value("a"))], rules).changes, []);
});

test("replace finds text as typed, ignoring case unless asked", () => {
  const replace = replacer("a.b", "$1", { caseSensitive: false, regex: false });
  assert.deepEqual(replace("xA.Bya.b"), { text: "x$1y$1", count: 2 });
  assert.deepEqual(replace("axb"), { text: "axb", count: 0 });
  const exact = replacer("a", "o", { caseSensitive: true, regex: false });
  assert.deepEqual(exact("Aa"), { text: "Ao", count: 1 });
  assert.equal(replacer("", "x", { caseSensitive: false, regex: false }), null);
});

test("replace with a regular expression uses its groups", () => {
  const replace = replacer("(\\w+)@(\\w+)", "$2 at $1", { caseSensitive: true, regex: true });
  assert.deepEqual(replace("ada@home, bob@work"), { text: "home at ada, work at bob", count: 2 });
  assert.ok("error" in replacer("(", "", { caseSensitive: true, regex: true }));
});

test("expressions the u flag refuses but the usual syntax takes still work", () => {
  const dash = replacer("\\-", "_", { caseSensitive: true, regex: true });
  assert.deepEqual(dash("a-b"), { text: "a_b", count: 1 });
  const word = replacer("[\\w-.]+", "x", { caseSensitive: true, regex: true });
  assert.deepEqual(word("a-b.c d"), { text: "x x", count: 2 });
});

test("replace changes only text cells, and counts the replacements", () => {
  const cells = [
    cell(0, 0, value("10"), "10"),
    cell(0, 1, value("banana"), "banana"),
    cell(1, 1, NULL, null),
    cell(2, 1, value("cherry"), "cherry"),
  ];
  const out = replaceText(cells, rules, replacer("a", "o", { caseSensitive: true, regex: false }));
  assert.equal(out.replacements, 3);
  assert.deepEqual(out.changes, [{ gridRow: 0, col: 1, value: value("bonono"), original: "banana" }]);
});

test("sequence values keep the decimals of start and step, and big whole numbers exact", () => {
  assert.equal(sequenceValue("1", "1", 0), "1");
  assert.equal(sequenceValue("10", "-5", 3), "-5");
  assert.equal(sequenceValue("0.1", "0.2", 2), "0.5");
  assert.equal(sequenceValue("1", "0.25", 3), "1.75");
  assert.equal(sequenceValue("9007199254740993", "2", 1), "9007199254740995");
  assert.equal(sequenceValue("+1", "1", 1), "2");
  assert.equal(sequenceValue("1e2", "1", 1), "101");
  assert.equal(sequenceValue("x", "1", 0), null);
});

test("a number sequence goes down each number column; other columns are reported", () => {
  const cells = [
    cell(2, 0, value("5"), "5"),
    cell(2, 1, value("a"), "a"),
    cell(0, 0, value("3"), "3"),
    cell(0, 2, value("1"), "1"),
    cell(1, 0, value("4"), "4"),
    cell(1, 2, NULL, null),
  ];
  const out = numberSequence(cells, rules, "100", "10");
  assert.deepEqual(
    out.changes.map((c) => [c.gridRow, c.col, c.value.text]),
    [
      [0, 0, "100"],
      [0, 2, "100"],
      [1, 0, "110"],
      [1, 2, "110"],
      [2, 0, "120"],
    ],
  );
  assert.deepEqual(out.rejected, [{ col: 1, problem: "not_number", cells: 1 }]);
  // A fraction does not fit a whole-number column.
  assert.deepEqual(numberSequence([cell(0, 0, value("1"), "1")], rules, "1", "0.5").rejected, [
    { col: 0, problem: "integer", cells: 1 },
  ]);
  assert.equal(numberSequence(cells, rules, "1", "x"), null);
});

test("ten thousand cells are computed quickly", () => {
  const cells = Array.from({ length: 10_000 }, (_, i) => cell(i, i % 2 ? 1 : 2, value(`v${i}`), `v${i}`));
  const started = performance.now();
  setValue(cells, rules, value("1"));
  fillDown(cells, rules);
  replaceText(cells, rules, replacer("v", "w", { caseSensitive: true, regex: false }));
  numberSequence(cells, rules, "1", "1");
  assert.ok(performance.now() - started < 500, "mass edits of 10,000 cells took too long");
});

test("the hex of a file of tens of megabytes is checked without overflowing the stack", () => {
  const bin = columnRule(info("bytea", { binary: true }), undefined);
  const big = "ab".repeat(30_000_000);
  assert.equal(checkValue(bin, value(`\\x${big}`)), null);
  assert.equal(checkValue(bin, value(`${big}0`)), "binary");
  assert.equal(checkValue(bin, value(`${big}zz`)), "binary");
  assert.ok(isHex("0xDEADbeef") && isHex("") && isHex("\\x"));
  // Control characters are not digits, whatever their bits.
  assert.ok(!isHex("\u0010\u0011") && !isHex("gg") && !isHex("0x0"));
});

test("selected cells read only the selected columns' values, in the selection's order", () => {
  const state = { inserts: [], updates: new Map(), deletes: new Set() };
  const cells = selectionCells(state, { r0: 0, r1: 0, cols: [2, 0] }, [["x", "1"]], 0);
  assert.deepEqual(cells, [cell(0, 2, value("x"), "x"), cell(0, 0, value("1"), "1")]);
});

test("regular expressions match whole characters, and changes that change nothing do not count", () => {
  const empty = replacer("x*", "-", { caseSensitive: true, regex: true });
  assert.equal(empty("😀").text, "-😀-");
  const none = replacer("x*", "", { caseSensitive: true, regex: true });
  assert.deepEqual(none("abc"), { text: "abc", count: 0 });
  const cells = [cell(0, 1, value("abc"), "abc")];
  assert.equal(replaceText(cells, rules, none).changes.length, 0);
});

test("a replacement run reports progress, and its results make the same outcome", () => {
  const cells = [cell(0, 1, value("banana"), "banana"), cell(0, 0, value("1"), "1"), cell(1, 1, value("kiwi"), "kiwi")];
  const { targets, texts } = replaceInputs(cells, rules);
  assert.deepEqual(targets, [0, 2]);
  const replies = [];
  let clock = 0;
  runReplaceJob({ texts, find: "a", replacement: "o", caseSensitive: true, regex: false }, (r) => replies.push(r), () => (clock += 200), 100);
  assert.deepEqual(replies.slice(0, 2), [{ done: 1 }, { done: 2 }]);
  const { results } = replies.at(-1);
  assert.deepEqual(replacedOutcome(cells, rules, targets, results), replaceText(cells, rules, replacer("a", "o", { caseSensitive: true, regex: false })));
  replies.length = 0;
  runReplaceJob({ texts, find: "(", replacement: "", caseSensitive: true, regex: true }, (r) => replies.push(r));
  assert.ok("error" in replies[0]);
});

/** A worker of `replaceWorker.ts`, run by Node: the same job runner, in a thread. */
function nodeWorker() {
  const runner = new URL("../src/grid/edit/massEdit.ts", import.meta.url).href;
  const code = `import { parentPort } from "node:worker_threads";
    import { runReplaceJob } from ${JSON.stringify(runner)};
    parentPort.on("message", (job) => runReplaceJob(job, (reply) => parentPort.postMessage(reply)));`;
  const worker = new Worker(new URL(`data:text/javascript,${encodeURIComponent(code)}`));
  const like = { onmessage: null, onerror: null, postMessage: (job) => worker.postMessage(job), terminate: () => void worker.terminate() };
  worker.on("message", (data) => like.onmessage?.({ data }));
  worker.on("error", (e) => like.onerror?.(e));
  return like;
}

test("an expression that backtracks without end is stopped, and the UI thread never runs it", async () => {
  const job = (find, texts) => ({ texts, find, replacement: "x", caseSensitive: true, regex: true });
  const ok = await runReplace(job("a+", ["aa", "b"]), nodeWorker, () => {}, 5000).result;
  assert.deepEqual(ok, { results: [{ text: "x", count: 1 }, null] });
  const started = performance.now();
  const slow = await runReplace(job("(a+)+b", ["a".repeat(60)]), nodeWorker, () => {}, 1500).result;
  assert.deepEqual(slow, { slow: true });
  assert.ok(performance.now() - started < 5000);
});

test("a silent worker is given up; a cancelled one settles nothing", async () => {
  const silent = () => ({ onmessage: null, onerror: null, postMessage() {}, terminate() {} });
  assert.deepEqual(await runReplace({ texts: [], find: "a", replacement: "", caseSensitive: true, regex: false }, silent, () => {}, 20).result, { slow: true });
  let terminated = false;
  const run = runReplace({ texts: [], find: "a", replacement: "", caseSensitive: true, regex: false }, () => ({ ...silent(), terminate: () => (terminated = true) }), () => {}, 20);
  run.cancel();
  const settled = await Promise.race([run.result.then(() => true), new Promise((r) => setTimeout(() => r(false), 60))]);
  assert.ok(terminated && !settled);
});

test("changes follow their rows when rows are added or dropped meanwhile", () => {
  const newRow = new Map([[1, value("n")]]);
  const base = { inserts: [newRow], updates: new Map(), deletes: new Set() };
  const changes = [
    { gridRow: 0, col: 1, value: value("a") },
    { gridRow: 1, col: 1, value: value("b"), original: "x" },
    { gridRow: 2, col: 1, value: value("c"), original: "y" },
  ];
  assert.deepEqual(rebase(base, base, changes), changes);
  // Two rows added on top, result row 1 deleted.
  const later = { inserts: [new Map(), new Map(), newRow], updates: new Map(), deletes: new Set([1]) };
  assert.deepEqual(
    rebase(base, later, changes).map((c) => c.gridRow),
    [2, 3],
  );
  // The new row was dropped (or edited, which makes it another map).
  const dropped = { inserts: [new Map(newRow)], updates: new Map(), deletes: new Set() };
  assert.deepEqual(
    rebase(base, dropped, changes).map((c) => [c.gridRow, c.value.text]),
    [
      [1, "b"],
      [2, "c"],
    ],
  );
});

test("sequences stay exact with many decimals or digits, and refuse what they cannot write", () => {
  assert.equal(sequenceValue("12345678901234567.1", "0.1", 1), "12345678901234567.2");
  assert.equal(sequenceValue(`0.${"0".repeat(149)}1`, "1", 1), `1.${"0".repeat(149)}1`);
  assert.equal(sequenceValue("-0.5", "0.25", 1), "-0.25");
  assert.equal(sequenceValue(".5", "1", 1), "1.5");
  assert.equal(sequenceValue("1e400", "1", 0), null);
  assert.equal(numberSequence([cell(0, 0, value("1"), "1")], rules, "1e400", "1"), null);
  // `1.0` is a whole number.
  assert.deepEqual(
    numberSequence([cell(0, 0, value("1"), "1"), cell(1, 0, value("2"), "2")], rules, "1.0", "1").changes.map((c) => c.value.text),
    ["1", "2"],
  );
});

test("DEFAULT is refused where it would be NULL in a NOT NULL column", () => {
  const notNull = columnRule(info("TEXT"), meta("TEXT", { nullable: false }));
  const out = setValue([cell(0, 0, value("a"), "a")], () => notNull, DEFAULT);
  assert.deepEqual(out.rejected, [{ col: 0, problem: "default", cells: 1 }]);
});
