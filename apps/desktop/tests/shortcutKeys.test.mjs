// Unit tests of shortcuts as text (src/shortcutKeys.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { bindingKey, keyName, labelParts, sameKey, splitShortcuts, typesText, withoutKeys } from "../src/shortcutKeys.ts";

test("the comma key has a name, and a stored list keeps it", () => {
  assert.equal(keyName(","), "Comma");
  assert.equal(keyName("+"), "Plus");
  assert.equal(keyName("k"), "K");
  assert.deepEqual(splitShortcuts("Mod+Shift+Comma"), ["Mod+Shift+Comma"]);
  // As stored before the key had a name.
  assert.deepEqual(splitShortcuts("Mod+Shift+,"), ["Mod+Shift+Comma"]);
  assert.deepEqual(splitShortcuts("Mod+,, Alt+X"), ["Mod+Comma", "Alt+X"]);
  assert.deepEqual(splitShortcuts("Mod+K, Mod+Shift+P"), ["Mod+K", "Mod+Shift+P"]);
  assert.deepEqual(splitShortcuts(""), []);
  assert.equal(sameKey(","), sameKey("Comma"));
});

test("labels show the characters and translated key names", () => {
  assert.deepEqual(labelParts("Mod+Shift+Comma"), ["Ctrl", "Shift", ","]);
  assert.deepEqual(labelParts("Alt+ArrowUp"), ["Alt", "Up"]);
  const it = { Shift: "Maiusc", Enter: "Invio" };
  assert.equal(labelParts("Mod+Shift+Enter", (k) => it[k] ?? k).join("+"), "Ctrl+Maiusc+Invio");
  // German keyboards write Ctrl as Strg, also for Mod.
  const de = { Ctrl: "Strg", Shift: "Umschalt" };
  assert.equal(labelParts("Mod+Shift+T", (k) => de[k] ?? k).join("+"), "Strg+Umschalt+T");
  assert.equal(labelParts("Ctrl+Space", (k) => de[k] ?? k).join("+"), "Strg+Space");
});

test("a key that types a character needs Ctrl or Alt", () => {
  assert.equal(typesText("Q"), true);
  assert.equal(typesText("Shift+Q"), true);
  assert.equal(typesText("Comma"), true);
  assert.equal(typesText("Mod+Q"), false);
  assert.equal(typesText("Alt+Q"), false);
  assert.equal(typesText("F2"), false);
  assert.equal(typesText("Delete"), false);
});

test("CodeMirror's and our spellings of a key compare equal", () => {
  assert.equal(bindingKey("Shift-Mod-k", false), bindingKey("Mod+Shift+K", false));
  assert.equal(bindingKey("Ctrl-Alt-[", false), bindingKey("Mod+Alt+[", false));
  assert.notEqual(bindingKey("Ctrl-Alt-[", true), bindingKey("Mod+Alt+[", true));
  assert.equal(bindingKey("Mod--", false), "ctrl+-");
  assert.equal(bindingKey("Mod+\\", false), bindingKey("Mod-\\", false));
});

test("bindings on the editor actions' keys give way", () => {
  const run = () => true;
  const shift = () => true;
  const taken = new Set(["Mod+Shift+K", "Alt+ArrowUp", "Shift+Alt+ArrowDown"].map((s) => bindingKey(s, false)));
  const kept = withoutKeys(
    [
      { key: "Shift-Mod-k", run },
      { key: "Mod-z", run },
      { key: "Alt-ArrowUp", run, shift },
      { key: "Alt-ArrowDown", run, shift },
      { key: "Mod-y", mac: "Mod-Shift-z", run },
    ],
    taken,
    false,
    true,
  );
  assert.deepEqual(
    kept.map((b) => [b.key, b.run === shift ? "shift" : "run", b.shift === undefined]),
    [
      ["Mod-z", "run", true],
      ["Shift-Alt-ArrowUp", "shift", true],
      ["Alt-ArrowDown", "run", true],
      ["Mod-y", "run", true],
    ],
  );
});

test("only a bare Backspace or Delete removes a shortcut while recording", async () => {
  const { removesShortcut } = await import("../src/shortcutKeys.ts");
  const press = (key, mods = {}) => ({ key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });
  assert.equal(removesShortcut(press("Delete")), true);
  assert.equal(removesShortcut(press("Backspace")), true);
  assert.equal(removesShortcut(press("Delete", { ctrlKey: true })), false);
  assert.equal(removesShortcut(press("Delete", { shiftKey: true })), false);
  assert.equal(removesShortcut(press("Backspace", { ctrlKey: true })), false);
  assert.equal(removesShortcut(press("a")), false);
});
