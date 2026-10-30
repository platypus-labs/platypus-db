// Every icon name has a matching icon in each family (src/ui/icons).
import assert from "node:assert/strict";
import { test } from "node:test";
import { LUCIDE } from "../src/ui/icons/lucide.ts";
import { PHOSPHOR } from "../src/ui/icons/phosphor.ts";
import { ICON_SETS } from "../src/ui/icons/sets.ts";
import { TABLER } from "../src/ui/icons/tabler.ts";

const renderable = (c) => typeof c === "function" || (typeof c === "object" && c !== null);

for (const [label, map] of [["Tabler", TABLER], ["Phosphor", PHOSPHOR]]) {
  test(`${label} has an icon for every name`, () => {
    for (const name of Object.keys(LUCIDE)) assert.ok(renderable(map[name]), `${label}: ${name}`);
    assert.deepEqual(Object.keys(map).sort(), Object.keys(LUCIDE).sort());
  });
}

test("the families are listed by proper name", () => {
  assert.deepEqual(ICON_SETS.map((s) => s.id), ["lucide", "tabler", "phosphor"]);
  assert.deepEqual(ICON_SETS.map((s) => s.label), ["Lucide", "Tabler", "Phosphor"]);
});
