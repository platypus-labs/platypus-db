// Unit tests of shortcuts as text (src/shortcutKeys.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  altGraphText,
  bindingKey,
  keyName,
  labelParts,
  matchesShortcut,
  onAltGraph,
  sameKey,
  shiftTyped,
  splitShortcuts,
  typesText,
  withoutKeys,
} from "../src/shortcutKeys.ts";

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

const key = (k, mods = {}) => ({ key: k, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

test("AltGr characters are text, also when Windows reports them as Ctrl+Alt", () => {
  // Italian AltGr+è, German AltGr+8 and AltGr+Q, French AltGr+E: Ctrl+Alt without the AltGraph state.
  for (const c of ["[", "]", "@", "#", "€", "{", "}", "\\", "|", "~", "ą"]) {
    assert.equal(altGraphText(key(c, { ctrlKey: true, altKey: true }), false), true, c);
  }
  // Linux reports the AltGraph state alone.
  assert.equal(altGraphText(key("[", { getModifierState: (m) => m === "AltGraph" }), false), true);
  // Ctrl+Alt with a Latin letter or digit, a named key or a plain symbol is not AltGr.
  assert.equal(altGraphText(key("d", { ctrlKey: true, altKey: true }), false), false);
  assert.equal(altGraphText(key("0", { ctrlKey: true, altKey: true }), false), false);
  assert.equal(altGraphText(key("ArrowUp", { ctrlKey: true, altKey: true }), false), false);
  assert.equal(altGraphText(key("[", { ctrlKey: true }), false), false);
  assert.equal(altGraphText(key("[", { altKey: true }), false), false);
  // macOS has no AltGr: Cmd+Option+[ is a shortcut.
  assert.equal(altGraphText(key("[", { ctrlKey: true, altKey: true }), true), false);
});

test("a shortcut never takes an AltGr character", () => {
  assert.equal(matchesShortcut("Mod+Alt+[", key("[", { ctrlKey: true, altKey: true }), false), false);
  assert.equal(matchesShortcut("Mod+Alt+[", key("[", { metaKey: true, altKey: true }), true), true);
  assert.equal(matchesShortcut("Mod+Alt+D", key("d", { ctrlKey: true, altKey: true }), false), true);
  assert.equal(matchesShortcut("Mod+Alt+ArrowUp", key("ArrowUp", { ctrlKey: true, altKey: true }), false), true);
  // German AltGr+M types µ: no shortcut on M runs.
  assert.equal(matchesShortcut("Mod+Alt+M", key("µ", { ctrlKey: true, altKey: true }), false), false);
});

test("a character typed with Shift matches its shortcut in the loose pass only", () => {
  // Italian `/` is Shift+7.
  const slash = key("/", { ctrlKey: true, shiftKey: true });
  assert.equal(matchesShortcut("Mod+/", slash, false), false);
  assert.equal(matchesShortcut("Mod+/", slash, false, true), true);
  assert.equal(matchesShortcut("Mod+Shift+/", slash, false), true);
  assert.equal(matchesShortcut("Mod+/", key("/", { ctrlKey: true }), false), true);
  // French digits need Shift; `+` needs it on US keyboards.
  assert.equal(matchesShortcut("Mod+1", key("1", { ctrlKey: true, shiftKey: true }), false, true), true);
  assert.equal(matchesShortcut("Mod+Plus", key("+", { ctrlKey: true, shiftKey: true }), false, true), true);
  // Shift makes a letter another shortcut.
  assert.equal(matchesShortcut("Mod+K", key("K", { ctrlKey: true, shiftKey: true }), false, true), false);
  assert.equal(shiftTyped("K"), false);
  assert.equal(shiftTyped("Comma"), true);
  assert.equal(shiftTyped("Space"), false);
  assert.equal(shiftTyped("ArrowUp"), false);
  assert.equal(shiftTyped("F2"), false);
});

test("CodeMirror bindings on AltGr characters give way outside macOS", () => {
  assert.equal(onAltGraph("Ctrl-Alt-[", false), true);
  assert.equal(onAltGraph("Mod-Alt-\\", false), true);
  assert.equal(onAltGraph("Mod-Alt-ArrowUp", false), false);
  assert.equal(onAltGraph("Ctrl-Shift-[", false), false);
  assert.equal(onAltGraph("Ctrl-Alt-[", true), false);
  const run = () => true;
  const bindings = [{ key: "Ctrl-Alt-[", run }, { key: "Mod-Alt-\\", run }, { key: "Mod-Alt-ArrowUp", run }];
  assert.deepEqual(withoutKeys(bindings, new Set(), false, true).map((b) => b.key), ["Mod-Alt-ArrowUp"]);
  assert.equal(withoutKeys(bindings, new Set(), true, false).length, 3);
});