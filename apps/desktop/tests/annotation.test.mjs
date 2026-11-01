// Unit tests of the navigator's #tag filter and the tooltip of annotated objects
// (src/catalog/annotation.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { addTags, annotation, parseFilter } from "../src/catalog/annotation.ts";

const entry = (note, tags = []) => ({ key: { database: "", schema: "public", object: "t", column: "" }, note, tags });

test("a #word that is not a known tag stays in the text", () => {
  const known = (t) => t === "pii";
  assert.deepEqual(parseFilter("#temp", known), { text: "#temp", tags: [] });
  assert.deepEqual(parseFilter("#pii #temp orders", known), { text: "#temp orders", tags: ["pii"] });
});

test("a filter without # is all text", () => {
  assert.deepEqual(parseFilter("orders"), { text: "orders", tags: [] });
  assert.deepEqual(parseFilter(""), { text: "", tags: [] });
  assert.deepEqual(parseFilter("   "), { text: "", tags: [] });
});

test("tags are split from the text, lower case, wherever they are", () => {
  assert.deepEqual(parseFilter("#pii orders"), { text: "orders", tags: ["pii"] });
  assert.deepEqual(parseFilter("orders #PII #Core"), { text: "orders", tags: ["pii", "core"] });
  assert.deepEqual(parseFilter("a #pii b"), { text: "a b", tags: ["pii"] });
  assert.deepEqual(parseFilter("#pii"), { text: "", tags: ["pii"] });
});

test("the text keeps its spacing", () => {
  assert.equal(parseFilter("order  items").text, "order  items");
  assert.equal(parseFilter("  order  items #pii ").text, "order  items");
});

test("a # alone or inside a word is plain text", () => {
  assert.deepEqual(parseFilter("#"), { text: "#", tags: [] });
  assert.deepEqual(parseFilter("a # b"), { text: "a # b", tags: [] });
  assert.deepEqual(parseFilter("table#1"), { text: "table#1", tags: [] });
  assert.deepEqual(parseFilter("##"), { text: "##", tags: [] });
});

test("extra leading # are dropped like the stored tags do", () => {
  assert.deepEqual(parseFilter("##pii"), { text: "", tags: ["pii"] });
});

test("nothing to say gives no tooltip", () => {
  assert.equal(annotation(undefined), undefined);
  assert.equal(annotation(undefined, null), undefined);
  assert.equal(annotation(undefined, ""), undefined);
  assert.equal(annotation(entry("", [])), undefined);
});

test("comment, note and tags come in that order, one per line", () => {
  assert.equal(
    annotation(entry("Check with finance", ["pii", "core"]), "Orders"),
    "Comment (database): Orders\nNote (only on this computer): Check with finance\n#pii #core",
  );
});

test("each part stands alone", () => {
  assert.equal(annotation(undefined, "Orders"), "Comment (database): Orders");
  assert.equal(annotation(entry("Mine")), "Note (only on this computer): Mine");
  assert.equal(annotation(entry("", ["pii"])), "#pii");
});

test("a long note shows its first line, unless the whole is asked for", () => {
  const long = entry("First\nSecond\nThird", ["pii"]);
  assert.equal(annotation(long), "Note (only on this computer): First…\n#pii");
  assert.equal(annotation(long, null, { full: true }), "Note (only on this computer): First\nSecond\nThird\n#pii");
  // Nothing follows the first line: no ellipsis.
  assert.equal(annotation(entry("First\n  \n")), "Note (only on this computer): First");
  assert.equal(annotation(entry("First\r\nSecond")), "Note (only on this computer): First…");
});

test("a typed tag is normalized and added in order", () => {
  assert.deepEqual(addTags(["pii"], "#Two Words"), ["pii", "two-words"]);
  assert.deepEqual(addTags([], "core"), ["core"]);
});

test("a pasted list adds every tag", () => {
  assert.deepEqual(addTags(["pii"], "b,a"), ["a", "b", "pii"]);
  assert.deepEqual(addTags([], " #a , b ,, "), ["a", "b"]);
});

test("nothing new leaves the tags as they are", () => {
  assert.deepEqual(addTags(["a", "b"], ""), ["a", "b"]);
  assert.deepEqual(addTags(["a", "b"], "#, ,"), ["a", "b"]);
  assert.deepEqual(addTags(["a", "b"], "B,a,a"), ["a", "b"]);
});

test("the labels come in the interface language", () => {
  const labels = { comment: "Commento (database)", note: "Nota (solo su questo computer)" };
  assert.equal(
    annotation(entry("Mia", ["pii"]), "Ordini", { labels }),
    "Commento (database): Ordini\nNota (solo su questo computer): Mia\n#pii",
  );
});
