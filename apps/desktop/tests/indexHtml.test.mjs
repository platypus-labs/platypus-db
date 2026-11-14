// index.html holds no inline <style> or <script>: Tauri adds a nonce for each to the content
// security policy, which turns off 'unsafe-inline', and CodeMirror's own style tags (the SQL
// editor's styles) are then blocked. The loading screen's styles and script are files in public/.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8").replace(/<!--[\s\S]*?-->/g, "");

test("index.html has no inline style element", () => {
  assert.doesNotMatch(html, /<style[\s>]/i);
});

test("every script of index.html is a file", () => {
  for (const tag of html.match(/<script\b[^>]*>/gi) ?? []) assert.match(tag, /\bsrc=/, tag);
});
