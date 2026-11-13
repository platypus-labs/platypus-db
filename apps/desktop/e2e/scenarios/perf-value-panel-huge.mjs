// BUG (high, freeze and memory blow-up): the value panel (F7) puts the whole value of the focused
// cell in one <pre> (src/panels/ValuePanel.tsx, the "text" and "hex" modes; hexDump builds a
// line per 16 bytes): a 6 MB PostgreSQL text took the WebView from 190 MB to 1.7 GB and froze
// the window 1.8 s; a 30 MB text peaked at 7 GB and froze it ~10 s (settling at 2.1 GB); a 4 MB
// bytea in hex mode adds ~250,000 lines. DBeaver caps what its value viewer loads (and offers
// to open/save the rest).
//   Steps: SELECT a 20 MB text cell, click it, F7.
//   Expected: the panel opens without freezing (main thread answers within 1.5 s) and without
//   holding gigabytes (WebView private memory grows by less than 1 GB).
//   Actual: the page does not answer for seconds and the WebView grows by several GB.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { gridAt, lag, memory, openEditor, run, saveConnections, sqliteEntry, visibleTab } from "../perf.mjs";

const NAME = "QA perf value panel";
const MB = 20;

export function setup({ dir }) {
  const file = join(dir, "huge.db");
  const db = new DatabaseSync(file);
  // Words with spaces, as a long document would have.
  db.exec(`CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT);
    INSERT INTO docs VALUES (1, replace(hex(zeroblob(${(MB * 1_000_000) / 2})), '000000000000', 'lorem ipsum '));`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page, { dir }) {
  await openEditor(page, NAME);
  await run(page, "SELECT id, body FROM docs");
  const before = memory(dir).webview;
  const g = await gridAt(page);
  await page.click(g.x + 160, g.y + 60);
  await page.key("F7", "F7", 118);
  let worstLag = 0;
  let peak = before;
  const started = performance.now();
  while (performance.now() - started < 12_000) {
    worstLag = Math.max(worstLag, await lag(page));
    peak = Math.max(peak, memory(dir).webview);
  }
  await page.waitFor(`document.querySelector('.value-panel') != null`);
  console.log(`  value panel on ${MB} MB: worst lag ${Math.round(worstLag)} ms, WebView ${before} -> peak ${peak} MB`);
  assert.ok(worstLag < 1500, `the window froze ${Math.round(worstLag)} ms`);
  assert.ok(peak - before < 1000, `the WebView grew from ${before} MB to ${peak} MB`);
  assert.ok(await page.eval(`${visibleTab} != null`));
}
