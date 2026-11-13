// QA ui: in a small window the window buttons are drawn over the Settings dialog.
//
// Steps: the window at its minimum size (800x500), open the Settings.
// Expected: the dialog fits under the top bar, or the backdrop covers the window buttons; the
// dialog's corner is not crossed by them.
// Actual: the dialog starts at the top of the window (about 18 px) and the minimize, maximize
// and close buttons sit over its top-right corner, next to the search field.
// Screenshot: small-800x500-z1.1-settings.png.
// Code: styles/shell.css `.window-controls { z-index: 60 }` (above every dialog, so the window
// can still be closed) with styles/dialogs.css `.modal.settings { height: min(720px, calc(100vh
// - 32px)) }`, which in a small window reaches under them.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page, { shots }) {
  await page.send("Emulation.setDeviceMetricsOverride", { width: 800, height: 500, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`innerWidth === 800`);
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.sleep(300);
  if (shots) await page.screenshot(join(shots, "ui-window-controls-overlap.png"));
  const overlap = await page.eval(`(() => {
    const m = document.querySelector('.modal.settings').getBoundingClientRect();
    return [...document.querySelectorAll('.window-controls button')].filter(b => {
      const r = b.getBoundingClientRect();
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      const inside = r.left < m.right && r.right > m.left && r.top < m.bottom && r.bottom > m.top;
      return inside && b.contains(hit);
    }).map(b => b.getAttribute('aria-label'));
  })()`);
  assert.deepEqual(overlap, [], "window buttons drawn over the dialog");
}
