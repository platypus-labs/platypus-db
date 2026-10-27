// QA ui: a context menu taller than the window is cut off; its last items cannot be reached.
//
// Steps: Interface size 125% on the default 1280x800 window (or any window under ~700 px of
// height at 100%); right-click a table in the navigator.
// Expected: the whole menu is reachable: it scrolls, or opens upward/with a max height.
// Actual: the menu (about 685 px at 100%) is moved to the top of the window and runs past its
// bottom edge; the items past it (Export data, Drop, Truncate, ...) cannot be clicked, and the
// menu does not scroll. Screenshot: 25-menu-small.png.
// Code: components/ContextMenu.tsx positions the menu but gives it no max-height/overflow.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA UI menu";

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_menu.db",
    id: "00000000-0000-4000-8000-00000000e117",
    name: NAME,
    sql: "CREATE TABLE qa_ui_orders (id INTEGER PRIMARY KEY);",
  });
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nzoom = "125"\n');
}

export default async function (page, { shots }) {
  await page.waitFor(`Math.abs(devicePixelRatio - 1.25) < 0.01`);
  // The window at its default size, whatever the screen running the test.
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`innerHeight === 640`);
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("qa_ui_orders")} !== undefined`, 15_000);
  const r = await page.eval(`(() => { const b = ${treeRow("qa_ui_orders")}.getBoundingClientRect(); return { x: b.x + 40, y: b.y + b.height / 2 }; })()`);
  await page.click(r.x, r.y, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.sleep(200);
  if (shots) await page.screenshot(join(shots, "qa-ui-context-menu-clipped.png"));
  const m = await page.eval(`(() => { const m = document.querySelector('.context-menu'); const r = m.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, vh: innerHeight, scrolls: m.scrollHeight > m.clientHeight && getComputedStyle(m).overflowY !== 'visible' }; })()`);
  assert.ok(m.top >= 0 && (m.bottom <= m.vh + 1 || m.scrolls), `menu from ${Math.round(m.top)} to ${Math.round(m.bottom)} px in a ${m.vh} px window, not scrollable`);
}
