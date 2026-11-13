// Guard: Cancel stops a long PostgreSQL statement at once (measured 66 ms; MySQL 237 ms, SQL
// Server 60 ms), the tab says "Query cancelled", and the session runs the next statement.
// Needs PostgreSQL (docker compose -f docker/compose.yml up -d).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { PG, expand, needsPort, openEditor, pgEntry, run, saveConnections, status, visibleTab } from "../perf.mjs";

export const skip = needsPort(PG.port, "PostgreSQL");
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;

export function setup({ dir }) {
  saveConnections(dir, pgEntry());
}

export default async function (page) {
  await expand(page, PG.name, PG.password);
  await openEditor(page, PG.name);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT pg_sleep(60)");
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes('pg_sleep(60)')`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${cancel} && !${cancel}.disabled`, 15_000);
  await page.sleep(1500);
  const started = performance.now();
  await page.eval(`${cancel}.click()`);
  await page.waitFor(`${cancel}.disabled`, 30_000);
  const took = performance.now() - started;
  console.log(`  cancel: ${Math.round(took)} ms`);
  assert.ok(took < 2000, `cancel took ${Math.round(took)} ms`);
  assert.match(await page.eval(`${visibleTab}.textContent`), /Query cancelled/);
  await run(page, "SELECT 42 AS answer");
  assert.equal(await page.eval(status), "1 row");
}
