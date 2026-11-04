// The user's reports of 3 November 2026: Ctrl+Enter runs only the statement under the cursor
// when the statements are apart by a blank line and have no `;`; a SQL editor shows no empty
// results panel until something runs; the first color swatch means no ring, whatever the
// connection's type; the connection dialog's check boxes keep their size beside a long label
// and take the pointer, and the Projects help sits beside its title; a field left invalid shows
// the app's message, not the browser's bubble.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { dialog, editConnection, fill, formField, saveForm } from "../conn-helpers.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";
import { openEditor } from "../qa-grid.mjs";
import { visibleTab } from "../qa-jrn.mjs";

const NAME = "S fix reports";
const ID = "00000000-0000-4000-8000-0000000ff101";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "reports.db", id: ID, name: NAME, sql: "CREATE TABLE a (x INTEGER); INSERT INTO a VALUES (1); CREATE TABLE b (y INTEGER); INSERT INTO b VALUES (2), (3);" });
  const file = join(dir, "connections.toml");
  const text = readFileSync(file, "utf8")
    .replace(/^version = \d+/, "version = 2")
    .replace(`name = "${NAME}"`, `name = "${NAME}"\nconnection_type = "dev"\ncolor = "#1c7ed6"`);
  writeFileSync(file, text.replace("[[connections]]", `[[projects]]\nid = "00000000-0000-4000-9000-0000000ff102"\nname = "Reports"\ncolor = "#0d9488"\nconnections = ["${ID}"]\n\n[[connections]]`));
}

const tab = visibleTab;
const editor = `${tab}.querySelector('.cm-content')`;
const resultTabs = `${tab}.querySelectorAll('.result-tab').length`;

export default async function (page) {
  await openEditor(page, NAME);

  // A new editor: no results panel, no handle, the editor down to the status bar.
  assert.equal(await page.eval(`${tab}.querySelector('.results') === null && ${tab}.querySelector('.query-main > .splitter') === null`), true, "no empty results panel");
  const gap = await page.eval(`Math.round(${tab}.querySelector('.statusbar').getBoundingClientRect().top - ${tab}.querySelector('.cm-editor').getBoundingClientRect().bottom)`);
  assert.ok(gap <= 2, `the editor reaches the status bar: ${gap} px between`);

  // Two statements without `;`, a blank line between: Ctrl+Enter on the second runs it alone.
  const sql = "select x from a\n\nselect y from b order by y";
  await page.eval(`${editor}.focus()`);
  await page.type(sql);
  await page.waitFor(`${editor}.textContent.includes('order by y')`);
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${tab}.querySelector('.results') !== null && ${tab}.querySelector('.statusbar span')?.textContent.includes('2 rows')`, 20_000);
  assert.equal(await page.eval(resultTabs), 0, "one statement ran: no result tabs");
  assert.equal(await page.eval(`${tab}.querySelector('.query-main > .splitter') !== null`), true, "the handle comes with the results");

  // On the first line, it runs the first one.
  await page.eval(`(() => { const s = ${tab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Home", "Home", 36, CTRL);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${tab}.querySelector('.statusbar span')?.textContent.includes('1 row')`, 20_000);
  assert.equal(await page.eval(resultTabs), 0);

  // The connection dialog: Appearance, Safety and Projects are on one page.
  await editConnection(page, NAME);
  await page.eval(`[...document.querySelectorAll('.connection-dialog .dialog-tabs [role=tab]')].find(t => t.textContent === 'Appearance and safety').click()`);
  await page.waitFor(`!document.querySelector('.connection-dialog [data-page=safety]').hidden`);

  const boxes = await page.eval(`[...document.querySelectorAll('.connection-dialog [data-page=safety] label.check > input[type=checkbox]')].map(b => {
    const r = b.getBoundingClientRect(); return { w: Math.round(r.width), h: Math.round(r.height), cursor: getComputedStyle(b).cursor, label: getComputedStyle(b.parentElement).cursor };
  })`);
  assert.ok(boxes.length >= 4, `check boxes on the page: ${boxes.length}`);
  for (const b of boxes) assert.deepEqual(b, { ...boxes[0], cursor: "pointer", label: "pointer" }, "every box the same size, with the pointer");

  const help = await page.eval(`(() => {
    const title = [...document.querySelectorAll('.connection-dialog h3')].find(h => h.textContent === 'Projects');
    const hint = title.nextElementSibling;
    const a = title.getBoundingClientRect(), b = hint.getBoundingClientRect();
    return { beside: b.left > a.right && Math.abs((b.top + b.bottom) / 2 - (a.top + a.bottom) / 2) < 8, text: hint.textContent };
  })()`);
  assert.deepEqual(help, { beside: true, text: "Show this connection in these projects." }, "the Projects help beside its title");

  // The first swatch: no ring, though the connection is a development one (green).
  const swatch = `document.querySelector('.connection-dialog .swatches .swatch')`;
  assert.equal(await page.eval(`${swatch}.title`), "No color");
  await page.eval(`${swatch}.click()`);
  await page.waitFor(`${swatch}.classList.contains('selected')`);
  await saveForm(page);
  const logo = `${treeRow(NAME)}.querySelector('.conn-logo')`;
  await page.waitFor(`!${logo}.classList.contains('colored')`);
  await editConnection(page, NAME);
  assert.equal(await page.eval(`document.querySelector('.connection-dialog .swatches .swatch').classList.contains('selected')`), true, "no color is kept");
  // A required field left empty: the app's message under it, not the browser's bubble (the
  // invalid event is taken), and typing ends it.
  const pop = `document.querySelector('.field-error-pop')`;
  await page.eval(`[...document.querySelectorAll('.connection-dialog .dialog-tabs [role=tab]')][0].click()`);
  await page.eval(fill(formField("Database file"), ""));
  await page.eval(`(() => { window.__nativeBubble = false; document.addEventListener('invalid', (e) => { if (!e.defaultPrevented) window.__nativeBubble = true; }, true); })()`);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`${pop} !== null`);
  const shown = await page.eval(`(() => {
    const p = ${pop}.getBoundingClientRect(), f = ${formField("Database file")}.getBoundingClientRect();
    return { text: ${pop}.textContent, under: p.top >= f.bottom && p.top - f.bottom < 12, marked: ${formField("Database file")}.classList.contains('field-invalid'),
      focused: document.activeElement === ${formField("Database file")}, native: window.__nativeBubble };
  })()`);
  assert.deepEqual(shown, { text: "Fill in this field.", under: true, marked: true, focused: true, native: false });
  assert.equal(await page.eval(`${dialog} !== null`), true, "not saved");
  await page.type("x");
  await page.waitFor(`${pop} === null && !${formField("Database file")}.classList.contains('field-invalid')`);

  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`${dialog} === null`);
}
