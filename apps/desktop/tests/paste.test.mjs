// Unit tests of the grid's clipboard text (src/grid/edit/paste.ts): what a paste splits into
// cells, how a copy writes them, and NULLs kept through the grid's own copy.
import assert from "node:assert/strict";
import { test } from "node:test";
import { clipboardText, isOwnCopy, mapFields, parseClipboard, rememberCopy } from "../src/grid/edit/paste.ts";

test("text without tabs or line breaks is one value, as it is", () => {
  for (const text of ["Paris, France", "a; b", '{"x": 1, "y": 2}', '"quoted" word', '"x"', "", "  spaced  "]) {
    assert.deepEqual(parseClipboard(text), [[text]]);
  }
});

test("tabs split cells and line breaks rows", () => {
  assert.deepEqual(parseClipboard("1\tParis, France\n2\ta;b"), [
    ["1", "Paris, France"],
    ["2", "a;b"],
  ]);
  assert.deepEqual(parseClipboard("a\r\nb\r\n"), [["a"], ["b"]]);
  assert.deepEqual(parseClipboard("a\t\nb\t"), [
    ["a", ""],
    ["b", ""],
  ]);
});

test("a cell in quotes keeps its tabs, line breaks and quotes", () => {
  assert.deepEqual(parseClipboard('"a\tb"\t"line 1\nline 2"\n"say ""hi"""\tx'), [
    ["a\tb", "line 1\nline 2"],
    ['say "hi"', "x"],
  ]);
  // A quote that does not close the cell is part of the value.
  assert.deepEqual(parseClipboard('"quoted" word\tx'), [['"quoted" word', "x"]]);
  assert.deepEqual(parseClipboard('"open\tx'), [['"open', "x"]]);
});

test("a copy writes one cell as it is and quotes cells that would split", () => {
  assert.equal(clipboardText([['"quoted"\tword']]), '"quoted"\tword');
  assert.equal(clipboardText([["a", null], ["", "b\nc"]]), 'a\t\n\t"b\nc"');
  assert.equal(clipboardText([['"x"', "y"]]), '"""x"""\ty');
  const rows = [['"x"', "a\tb", "c"]];
  assert.deepEqual(parseClipboard(clipboardText(rows) + "\n"), rows);
});

test("the grid's own copy comes back with its NULLs", () => {
  const rows = [["", null], ["b", "c"]];
  const text = clipboardText(rows);
  rememberCopy(text, rows);
  assert.ok(isOwnCopy(text));
  // Clipboards may add a line break or turn them into CRLF.
  assert.ok(isOwnCopy(`${text.replace(/\n/g, "\r\n")}\r\n`));
  assert.deepEqual(parseClipboard(text), rows);
  assert.ok(!isOwnCopy("other"));
  assert.deepEqual(parseClipboard("\t"), [["", ""]]);
});

test("a first row of column names maps the fields by name", () => {
  const names = ["id", "name", "note"];
  assert.deepEqual(mapFields([["note", "ID"], ["x", "1"]], names, [0, 1, 2]), { targets: [2, 0], rows: [["x", "1"]] });
  assert.deepEqual(mapFields([["a", null]], names, [1, 2]), { targets: [1, 2], rows: [["a", null]] });
});
