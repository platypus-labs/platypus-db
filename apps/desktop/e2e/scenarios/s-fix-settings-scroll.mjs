// The Settings scroll without lag: on every page, the mouse wheel keeps the frames short
// (with the glass blurred behind the dialog, a frame took twice as long: 22-28 ms).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { startFrames, stopFrames } from "../perf.mjs";

export const ready = `document.querySelector('.shell') !== null`;

const body = `document.querySelector('.settings-body')`;

/** Turns the wheel over the page `times` times, a notch a frame, as a hand does. */
async function wheel(page, times) {
  const r = await page.eval(`(() => { const r = ${body}.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  for (let i = 0; i < times; i++) {
    await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: r.x, y: r.y, deltaX: 0, deltaY: 100 });
  }
}

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  const sections = await page.eval(`[...document.querySelectorAll('.modal.settings [role=tab][data-section]')].map(t => t.dataset.section)`);
  const worst = [];
  for (const section of sections) {
    await page.settingsTab(section);
    if (await page.eval(`${body}.scrollHeight - ${body}.clientHeight < 100`)) continue;
    await page.eval(`${body}.scrollTop = 0`);
    await startFrames(page);
    await wheel(page, 30);
    const s = await stopFrames(page);
    const top = await page.eval(`${body}.scrollTop`);
    console.log(`${section}: ${s.frames} frames, p95 ${s.p95} ms, max ${s.max} ms, scrolled ${top} px`);
    assert.ok(top > 0, `${section}: the wheel scrolls the page`);
    worst.push({ section, ...s });
  }
  assert.ok(worst.length > 0, "a page long enough to scroll");
  const slow = worst.filter((s) => s.p95 > 18);
  assert.deepEqual(slow, [], "frames while the wheel scrolls the Settings");
}
