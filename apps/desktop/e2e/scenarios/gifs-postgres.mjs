// The README's GIFs that need the dev PostgreSQL from docker/compose.yml: a table of a million
// rows opened from the navigator, scrolled and sorted from its header; then Explain analyze
// with the heatmap and the index suggestion. Runs only with PLATYPUS_README_GIFS set to the
// folder they go to (docs/screenshots), and with the database running.
import { join, resolve } from "node:path";
import { CTRL } from "../cdp.mjs";
import { pointAndClick, pointAt, Recorder } from "../gif.mjs";
import { skip as noPostgres } from "./dev-postgres.mjs";
import { setup as localPostgres } from "./readme-postgres.mjs";
import { prepare, setEditor, status, treeRow, visibleTab } from "./readme-sqlite.mjs";

export async function skip() {
  return process.env.PLATYPUS_README_GIFS ? noPostgres() : "PLATYPUS_README_GIFS not set";
}

export const setup = localPostgres;

const NAME = "Local PG";
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const mark = (expr, id) => `(() => { const e = ${expr}; if (e) e.id = ${JSON.stringify(id)}; return !!e; })()`;

async function signIn(page) {
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`, 15_000);
  await page.waitFor(`${treeRow("demo")} !== undefined && ${treeRow("public")} !== undefined`, 15_000);
  if (await page.eval(`${treeRow("public")}.getAttribute('aria-expanded') === 'true'`)) {
    await page.eval(`${treeRow("public")}.querySelector(".twisty").click()`);
  }
}

/** Wheel turns over the grid, `delta` pixels each. */
async function wheel(page, x, y, turns, delta) {
  for (let i = 0; i < turns; i++) {
    await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY: delta });
    await page.sleep(60);
  }
}

/** A million rows: opened from the navigator, scrolled through, sorted from the header. */
async function heroGif(page, out) {
  const gif = new Recorder(page);
  await gif.start();
  await gif.hold(600);
  await page.eval(mark(`${treeRow("demo")}?.querySelector('.twisty')`, "e2e-demo"));
  await pointAndClick(page, "#e2e-demo");
  await page.waitFor(`${treeRow("big_table")} !== undefined`, 15_000);
  await gif.hold(400);
  await page.eval(mark(`${treeRow("big_table")}?.querySelector('.tree-label')`, "e2e-big"));
  await pointAndClick(page, "#e2e-big", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('big_table')`, 15_000);
  await page.waitFor(`/rows/.test(${status})`, 30_000);
  await gif.hold(1000);
  const box = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, left: r.x, top: r.y }; })()`);
  await pointAt(page, box);
  // Fast down through the rows as they arrive, then back up a little.
  await wheel(page, box.x, box.y, 30, 2400);
  await wheel(page, box.x, box.y, 25, 6000);
  await gif.hold(600);
  await wheel(page, box.x, box.y, 10, -1500);
  await gif.hold(800);
  // A click on a header sorts on the server and writes the ORDER BY in the editor.
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await pointAndClick(page, { x: box.left + 50, y: box.top + 14 });
  await page.waitFor(`/rows/.test(${status})`, 30_000);
  await gif.hold(800);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await pointAndClick(page, { x: box.left + 50, y: box.top + 14 });
  await page.waitFor(`/rows/.test(${status})`, 30_000);
  await gif.hold(2000);
  const size = await gif.save(join(out, "hero.gif"));
  console.log(`hero.gif: ${(size / 1024).toFixed(0)} KB`);
  await page.key("w", "KeyW", 87, CTRL);
}

/** Explain analyze of a slow join: the heatmap, then the index it suggests. */
async function explainGif(page, out) {
  await page.eval(mark(treeRow(NAME), "e2e-connection"));
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-connection')?.removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await setEditor(
    page,
    "SELECT c.country, count(*) AS orders, sum(o.total) AS revenue\nFROM demo.orders o\n" +
      "JOIN demo.customers c ON c.id = o.customer_id\nWHERE o.status = 'paid'\nGROUP BY c.country\nORDER BY revenue DESC",
  );
  await page.key("Escape", "Escape", 27);
  const gif = new Recorder(page);
  await gif.start();
  await gif.hold(1200);
  await page.eval(`${visibleTab}.querySelector('.cm-editor').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await pointAndClick(page, { x: Math.round(r.x + r.w / 2), y: Math.round(r.y + 20) }, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  const item = `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Explain analyze'))`;
  await page.eval(mark(item, "e2e-explain"));
  await pointAndClick(page, "#e2e-explain");
  await page.waitFor(`${explainTab}?.querySelector('.explain-suggestion') != null`, 15_000);
  await gif.hold(2500);
  await page.eval(mark(`${explainTab}.querySelector('.explain-suggestion')`, "e2e-suggestion"));
  await pointAt(page, "#e2e-suggestion");
  await gif.hold(2500);
  const size = await gif.save(join(out, "explain.gif"));
  console.log(`explain.gif: ${(size / 1024).toFixed(0)} KB`);
}

export default async function (page) {
  const out = resolve(process.env.PLATYPUS_README_GIFS);
  await prepare(page);
  await signIn(page);
  await heroGif(page, out);
  await explainGif(page, out);
}
