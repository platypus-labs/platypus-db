// Editing a column comment from the object tab, against the dev PostgreSQL from
// docker/compose.yml. Skipped when it is not running.
import assert from "node:assert/strict";
import { ALT, CTRL } from "../cdp.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const noteCell = `[...${objectTab}.querySelectorAll('.meta-table tbody tr')].find(r => r.children[1].textContent === 'note').children[6]`;

/** Replaces the editor's text and runs it as a script. */
async function run(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-20))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("x", "KeyX", 88, ALT);
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 15_000);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);

  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await run(page, "DROP TABLE IF EXISTS demo.e2e_comments;\nCREATE TABLE demo.e2e_comments (id int, note text);");

  // The new table appears in the navigator after a refresh of the schema.
  await page.eval(`${row("demo")}.querySelector(".twisty").click()`);
  await page.eval(`${row("demo")}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("e2e_comments")} !== undefined`, 15_000);
  await page.eval(`${row("e2e_comments")}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Open details').click()`,
  );
  await page.waitFor(`${objectTab} != null`, 15_000);
  await page.eval(`[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Columns')).click()`);
  await page.waitFor(`(() => { try { return ${noteCell}.textContent === 'Edit'; } catch { return false; } })()`, 15_000);

  await page.eval(`${noteCell}.querySelector('button').click()`);
  await page.waitFor(`document.querySelector('.sql-action input') !== null`);
  await page.eval(`document.querySelector('.sql-action input').focus()`);
  await page.type("free text");
  await page.waitFor(`document.querySelector('.sql-action input').value === 'free text'`);
  await page.waitFor(`document.querySelector('.sql-preview').textContent.includes('COMMENT ON COLUMN')`);
  await page.eval(`[...document.querySelectorAll('.sql-action button')].find(b => b.textContent === 'Save comment').click()`);
  await page.waitFor(`document.querySelector('.sql-action') === null`);
  await page.waitFor(`(() => { try { return ${noteCell}.textContent.includes('free text'); } catch { return false; } })()`, 15_000);

  // A new query tab for the clean-up.
  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await run(page, "DROP TABLE demo.e2e_comments;");
}
