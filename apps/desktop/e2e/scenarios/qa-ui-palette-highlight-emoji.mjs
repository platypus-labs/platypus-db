// QA ui: the command palette bolds the wrong letters after an emoji or other astral character.
//
// Steps: a project named "🦆 Ducks"; Ctrl+K, type "ducks".
// Expected: "Ducks" in bold.
// Actual: "ucks" in bold, one letter late: fuzzy() returns UTF-16 indices, Highlight walks
// code points (`[...text]`), and the emoji is two UTF-16 units but one code point.
// Code: shell/fuzzy.ts (positions from `toLowerCase().indexOf`) and shell/CommandPalette.tsx
// Highlight.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

export function setup({ dir }) {
  const path = join(dir, "qa_ui_palette.db");
  new DatabaseSync(path).close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "00000000-0000-4000-9000-00000000e111"
name = "🦆 Ducks"
color = "#d08f20"
connections = []

[[connections]]
id = "00000000-0000-4000-8000-00000000e111"
name = "QA UI palette"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

export default async function (page) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') === document.activeElement`);
  await page.type("ducks");
  const item = `[...document.querySelectorAll('.palette li')].find(l => l.querySelector('.palette-label')?.textContent === '🦆 Ducks')`;
  await page.waitFor(`${item} !== undefined`);
  assert.equal(await page.eval(`[...${item}.querySelectorAll('.palette-label b')].map(b => b.textContent).join('')`), "Ducks");
}
