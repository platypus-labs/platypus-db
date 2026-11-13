// Regression guard (passes): a long export cancelled from the status bar leaves no file (nor
// its ".part"), an Excel export of more rows than a sheet holds is refused without a file, and
// the export of "All rows" runs the query again with the tab's @set variables.
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { answerSave, button, DBS, exportResult, openEditor, runScript, saveConnections, skipUnless, visibleTab } from "../transfer-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

const outcome = `[...${visibleTab}.querySelectorAll('span')].find(s => /^(Exported|Export failed|Export cancelled)/.test(s.textContent))`;

export default async function (page, { dir }) {
  const out = join(dir, "out");
  mkdirSync(out);
  await openEditor(page, DBS.pg.name);

  assert.equal(await runScript(page, "@set n = 3\nSELECT generate_series(1, ${n}) AS g"), "3 rows");
  assert.match(await exportResult(page, join(out, "vars.csv")), /^Exported 3 rows/);
  assert.equal(readFileSync(join(out, "vars.csv"), "utf8"), "g\n1\n2\n3\n");

  await runScript(page, "SELECT g FROM generate_series(1, 1048576) g");
  assert.match(await exportResult(page, join(out, "big.xlsx"), { format: "xlsx" }), /at most 1048575 rows/);
  assert.deepEqual(readdirSync(out), ["vars.csv"]);

  await runScript(page, "SELECT g, md5(g::text) AS h, repeat('x', 200) AS pad FROM generate_series(1, 3000000) g");
  await page.eval(`[...${visibleTab}.querySelectorAll('button.link')].find(b => b.textContent === 'Export…').click()`);
  const dialog = `[...document.querySelectorAll('.modal')].find(m => m.querySelector('h2')?.textContent === 'Export result')`;
  await page.waitFor(`${dialog} !== undefined`);
  await answerSave(page, join(out, "long.csv"));
  await page.eval(`${button(dialog, "Export…")}.click()`);
  const cancel = `[...${visibleTab}.querySelectorAll('span button.link')].find(b => b.textContent === 'Cancel')`;
  await page.waitFor(`${cancel} !== undefined`, 20_000);
  await page.waitFor(`/Exporting… [1-9]/.test(${visibleTab}.textContent)`, 20_000);
  await page.eval(`${cancel}.click()`);
  await page.waitFor(`${outcome} !== undefined`, 30_000);
  assert.equal(await page.eval(`${outcome}.textContent.replace('×', '')`), "Export cancelled");
  await page.sleep(1000);
  assert.deepEqual(readdirSync(out), ["vars.csv"]);
}
