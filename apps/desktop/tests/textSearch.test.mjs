// Unit tests of the search fields' matching (src/ui/textSearch.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { fold, startsWord } from "../src/ui/textSearch.ts";

test("folding ignores case and accents", () => {
  assert.equal(fold("Città Élite"), "citta elite");
});

test("a search matches at the start of a word, not inside one", () => {
  assert.equal(startsWord("Tabs", "tab"), true);
  assert.equal(startsWord("Group tabs by connection", "tab"), true);
  assert.equal(startsWord("Simple shows only the tables", "tab"), true);
  assert.equal(startsWord("Messages from the databases stay in theirs", "tab"), false);
  assert.equal(startsWord("Comfortable", "tab"), false);
});

test("words start after punctuation, and a later occurrence can match", () => {
  assert.equal(startsWord("editor.font_size", "size"), true);
  assert.equal(startsWord("results.timezone", "timezone"), true);
  assert.equal(startsWord("database tables", "tab"), true);
});

test("several words match as a phrase, and accents in the text are ignored", () => {
  assert.equal(startsWord("Reopen closed tab", "reopen closed"), true);
  assert.equal(startsWord("Lingua dell'interfaccia: Città", "citta"), true);
  assert.equal(startsWord("anything", ""), true);
});
