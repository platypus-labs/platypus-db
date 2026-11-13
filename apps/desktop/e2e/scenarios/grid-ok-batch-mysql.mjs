// Regression guard (passes): a thousand rows changed at once on MySQL. A value set on every
// row (with the cells a column cannot take reported), all rows deleted, then a thousand rows
// pasted as new rows: each save is quick and leaves the table as expected.
import { spawnSync } from "../proc.mjs";
import { CTRL, SHIFT, assert, button, connections, editBar, expand, focusCell, grid, gridMenu, needs, openData, save, sh } from "../grid-helpers.mjs";

export const skip = needs("mysql");
const N = 1000;
/** Generous: catches one round trip per row, not a slow machine. */
const SAVE_LIMIT_MS = 10_000;

export function setup({ dir }) {
  sh("mysql", `DROP TABLE IF EXISTS qa_grid_ok_batch; CREATE TABLE qa_grid_ok_batch (id int PRIMARY KEY, v text, w int);
    INSERT INTO qa_grid_ok_batch WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < ${N}) SELECT x, CONCAT('v', x), x FROM c;`);
  connections(dir, [["QA MY", "mysql"]]);
}

async function timedSave(page) {
  const started = performance.now();
  assert.match(await save(page), /to edit a cell/);
  const ms = performance.now() - started;
  assert.ok(ms < SAVE_LIMIT_MS, `save took ${Math.round(ms)} ms`);
}

export default async function (page) {
  try {
    await expand(page, "QA MY", "mysql", "qa_grid_ok_batch");
    await openData(page, "qa_grid_ok_batch");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);

    await page.eval(`${grid}.focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await gridMenu(page, "Edit selection", "Set value…");
    await page.waitFor(`document.querySelector('.mass-edit') !== null`);
    await page.eval(`(() => {
      const el = document.querySelectorAll('.mass-edit input:not([type])')[0];
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, 'same');
      el.dispatchEvent(new Event('input', { bubbles: true }));
    })()`);
    await page.waitFor(`(document.querySelector('.mass-edit-summary')?.textContent ?? '').includes('1,000') || (document.querySelector('.mass-edit-summary')?.textContent ?? '').includes('1000')`);
    await page.eval(`${button("document.querySelector('.mass-edit')", "Apply")}.click()`);
    await page.waitFor(`${editBar}.includes('rows changed') && ${editBar}.includes('not a whole number')`, 30_000);
    await timedSave(page);
    assert.equal(sh("mysql", "SELECT count(*), sum(v = 'same') FROM qa_grid_ok_batch"), "1000\t1000");

    await page.eval(`${grid}.focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.key("Delete", "Delete", 46);
    await page.waitFor(`${editBar}.replace(/\\D/g, '').startsWith('1000')`);
    await timedSave(page);
    assert.equal(sh("mysql", "SELECT count(*) FROM qa_grid_ok_batch"), "0");

    const text = Array.from({ length: N }, (_, i) => `${i + 1}\tp${i}\t${i}`).join("\r\n");
    const r = spawnSync("powershell", ["-NoProfile", "-Command", "Set-Clipboard -Value $env:E2E_CLIP"], { env: { ...process.env, E2E_CLIP: text } });
    assert.equal(r.status, 0);
    await focusCell(page, 0, 0);
    await page.key("V", "KeyV", 86, CTRL | SHIFT);
    await page.waitFor(`${editBar}.replace(/\\D/g, '').startsWith('1000')`, 30_000);
    await timedSave(page);
    assert.equal(sh("mysql", "SELECT count(*), sum(v LIKE 'p%') FROM qa_grid_ok_batch"), "1000\t1000");
  } finally {
    sh("mysql", "DROP TABLE IF EXISTS qa_grid_ok_batch");
  }
}
