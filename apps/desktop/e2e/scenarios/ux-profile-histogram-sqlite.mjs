// UX (group T): the table profile's histogram has a title, a one-line explanation, a row-count
// scale, the first and last bound under the bars and a tooltip per bar; the bounds and the
// min/max of a timestamp column never overlap and stay inside their card.
import assert from "node:assert/strict";
import { liteConnection, treeRow } from "../qa-ana.mjs";

const NAME = "UX profile histogram";
const tab = `[...document.querySelectorAll('.table-profile-tab')].find(t => t.style.display !== 'none')`;
const card = (col) => `${tab}.querySelector('.tp-card[data-column="${col}"]')`;

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000b0f01",
    `CREATE TABLE ev (id INTEGER PRIMARY KEY, amount INTEGER, created_at TIMESTAMP);
     WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 300)
       INSERT INTO ev SELECT n, n * 7, strftime('%Y-%m-%d %H:%M:%f', '2026-01-01 00:00:00', '+' || n || ' hours') || '+00:00 Europe/Rome' FROM g;`,
  );
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("ev")} !== undefined`, 15_000);
  await page.eval(`${treeRow("ev")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Profile data…').click()`);
  await page.waitFor(`${tab}?.querySelector('.tp-status')?.textContent.includes('300 rows read')`, 30_000);
  await page.waitFor(`${card("amount")}?.querySelector('.column-profile-hist') !== null`);

  // The numeric column's histogram: title, explanation, scale, bounds, one tooltip per bar.
  const hist = await page.eval(`(() => {
    const h = ${card("amount")}.querySelector('.column-profile-hist');
    return {
      title: h.querySelector('.column-profile-hist-title strong').textContent,
      help: h.querySelector('.column-profile-hist-title .muted').textContent,
      axis: [...h.querySelectorAll('.column-profile-axis span')].map(s => s.textContent),
      range: [...h.querySelectorAll('.column-profile-range span')].map(s => s.textContent),
      bars: [...h.querySelectorAll('rect title')].map(t => t.textContent),
      rects: h.querySelectorAll('rect').length,
    };
  })()`);
  assert.equal(hist.title, "Distribution");
  assert.match(hist.help, /How many rows fall in each range of values/);
  assert.equal(hist.axis[1], "0", "the y axis starts at 0");
  assert.match(hist.axis[0], /^\d/, "the y axis ends at the largest bar count");
  assert.equal(hist.range.length, 2, "first and last bound under the bars");
  assert.ok(hist.range[0] !== "" && hist.range[1] !== "", "bounds are not empty");
  assert.equal(hist.bars.length, hist.rects, "every bar has a tooltip");
  assert.ok(hist.bars.every((b) => /: [\d,]+ rows?$/.test(b)), `bar tooltips: ${hist.bars[0]}`);

  // The timestamp column: nothing overlaps and everything stays in the card.
  const layout = await page.eval(`(() => {
    const c = ${card("created_at")};
    const box = c.getBoundingClientRect();
    const els = [...c.querySelectorAll('.tp-min, .tp-max, .column-profile-range span')];
    const rects = els.map(e => { const r = e.getBoundingClientRect(); return { cls: e.className || 'range', l: r.left, r: r.right, t: r.top, b: r.bottom, w: r.width }; });
    const overlaps = [];
    for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) {
      const a = rects[i], b = rects[j];
      if (a.w > 0 && b.w > 0 && a.l < b.r - 0.5 && b.l < a.r - 0.5 && a.t < b.b - 0.5 && b.t < a.b - 0.5) overlaps.push(a.cls + '/' + b.cls);
    }
    const outside = rects.filter(r => r.l < box.left - 1 || r.r > box.right + 1 || r.t < box.top - 1 || r.b > box.bottom + 1).map(r => r.cls);
    const hasMinMax = c.querySelector('.tp-min') !== null && c.querySelector('.tp-max') !== null;
    return { overlaps, outside, hasMinMax, n: rects.length };
  })()`);
  assert.ok(layout.hasMinMax, "the timestamp column shows min and max");
  assert.deepEqual(layout.overlaps, [], "min/max and bounds do not overlap");
  assert.deepEqual(layout.outside, [], "min/max and bounds stay inside the card");

  // A long value keeps its full text in the title when it is cut.
  const titled = await page.eval(`(() => {
    const e = ${card("created_at")}.querySelector('.tp-max');
    return e.title === e.textContent && e.title.length > 0;
  })()`);
  assert.ok(titled);
}
