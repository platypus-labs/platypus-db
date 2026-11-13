// QA ui: the navigator stays narrowed after the window shrinks and grows back.
//
// Steps: widen the navigator to 500 px (drag or arrow keys on its handle), make the window
// small (800 px wide), then maximize it again.
// Expected: the navigator returns to 500 px, the width saved in settings.toml (and the one
// the next start shows).
// Actual: it stays at the narrowed width (380 px) until restarted or resized by hand.
// Code: shell/SidebarResizer.tsx useSidebarWidth, the resize listener clamps the current
// state (`setWidth((w) => clamp(w))`) instead of the saved width.
import assert from "node:assert/strict";
import { sqliteConnection, treeRow } from "../query.mjs";
import { settingsFile } from "../settings.mjs";

const NAME = "QA UI sidebar";
const width = `Math.round(document.querySelector('.sidebar-column').getBoundingClientRect().width)`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_sidebar.db",
    id: "00000000-0000-4000-8000-00000000e118",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`innerWidth === 1280`);
  await page.eval(`document.querySelector('.sidebar-resizer').focus()`);
  // 260 + 15 steps of 16 px.
  for (let i = 0; i < 15; i++) await page.key("ArrowRight", "ArrowRight", 39);
  await page.waitFor(`${width} === 500`);
  await settingsFile(dir, /sidebar_width = 500/);

  await page.send("Emulation.setDeviceMetricsOverride", { width: 800, height: 600, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`innerWidth === 800`);
  await page.waitFor(`${width} < 500`);
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`innerWidth === 1280`);
  await page.sleep(300);
  assert.equal(await page.eval(width), 500);
}
