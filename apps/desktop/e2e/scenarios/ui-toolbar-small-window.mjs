// QA ui: the SQL editor's toolbar collapses in the smallest window at a large interface size.
//
// Steps: Interface size 150%, window at its minimum size (800x500, tauri.conf.json), open a
// SQL editor.
// Expected: the toolbar keeps its height; Run, Script, Cancel... stay usable (wrapped, or in
// an overflow menu).
// Actual: the toolbar shrinks to about 14 px (its flex item shrinks under the editor's fixed
// height): every button is cut, with only a horizontal scrollbar left. Even at 100% the
// minimum window hides Terminal, Variables, Outline and History behind a scrollbar.
// Screenshot: small-800x500-z1.5.png.
// Code: components/QueryTab.tsx, `gridTemplateRows: auto ... ${editorHeight}px 6px 1fr auto`
// keeps the editor at a fixed height; the toolbar (styles/editor.css, `overflow-x: auto`) has
// no minimum height, so its auto row gives way first.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { sqliteConnection, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA UI small";

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_small.db",
    id: "00000000-0000-4000-8000-00000000e110",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nzoom = "150"\n');
}

export default async function (page, { shots }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.toolbar') != null`);
  await page.waitFor(`Math.abs(devicePixelRatio - 1.5) < 0.01`);
  // The window at its minimum size.
  await page.send("Emulation.setDeviceMetricsOverride", { width: 800, height: 500, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`innerWidth < 600`);
  await page.sleep(300);
  if (shots) await page.screenshot(join(shots, "ui-toolbar-small-window.png"));
  const clipped = await page.eval(`(() => {
    const tb = ${visibleTab}.querySelector('.toolbar');
    const r = tb.getBoundingClientRect();
    return [...tb.querySelectorAll('button')].filter(b => b.getClientRects().length).filter(b => {
      const x = b.getBoundingClientRect();
      return x.top < r.top - 0.5 || x.bottom > r.bottom + 0.5;
    }).map(b => b.getAttribute('aria-label') || b.textContent);
  })()`);
  assert.deepEqual(clipped, [], `toolbar buttons cut at the top or bottom; toolbar height ${await page.eval(`${visibleTab}.querySelector('.toolbar').clientHeight`)}px`);
}
