// The user's reports of 2 November 2026: the SQL filter of a result keeps its whole border (the
// one-line editor fits inside it), the top bar shows the app's own icon, a connection's color is
// a ring around its logo (no corner dot), and toasts stack without the notice margin.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { sqliteConnection, treeRow } from "../query.mjs";
import { openData, openEditor } from "../qa-grid.mjs";

const NAME = "S fix UI";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "ui.db", id: "00000000-0000-4000-8000-0000000ff001", name: NAME, sql: "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);" });
  const file = join(dir, "connections.toml");
  writeFileSync(file, readFileSync(file, "utf8").replace(`name = "${NAME}"`, `name = "${NAME}"\ncolor = "#e5484d"`));
}

export default async function (page) {
  await openEditor(page, NAME, null, "t");
  await openData(page, "t");

  const fit = await page.eval(`(() => {
    const field = document.querySelector('.query-tab:not([style*="none"]) .result-sql-field');
    const box = field.getBoundingClientRect();
    const editor = field.querySelector('.cm-editor').getBoundingClientRect();
    return { top: editor.top >= box.top + 1, bottom: editor.bottom <= box.bottom - 1, bg: getComputedStyle(field.querySelector('.cm-editor')).backgroundColor };
  })()`);
  assert.deepEqual(fit, { top: true, bottom: true, bg: "rgba(0, 0, 0, 0)" }, "the filter's editor sits inside the field's border");

  assert.match(await page.eval(`document.querySelector('.topbar-mark').getAttribute('src')`), /platypus-icon/);

  const logo = `${treeRow(NAME)}.querySelector('.conn-logo')`;
  assert.equal(await page.eval(`${logo}.classList.contains('colored') && ${logo}.querySelector('.conn-color') === null`), true);
  assert.match(await page.eval(`getComputedStyle(${logo}.querySelector('.driver-logo')).boxShadow`), /rgb\(229, 72, 77\)/);

  // Two toasts: only the stack's gap between them.
  await page.eval(`(() => {
    const t = document.createElement('div');
    t.className = 'toaster';
    t.innerHTML = '<div class="notice error toast">a</div><div class="notice error toast">b</div>';
    document.body.appendChild(t);
  })()`);
  const gap = await page.eval(`(() => { const [a, b] = document.querySelectorAll('.toaster:last-child .toast'); return Math.round(b.getBoundingClientRect().top - a.getBoundingClientRect().bottom); })()`);
  assert.ok(gap <= 8, `toasts ${gap} px apart`);
}
