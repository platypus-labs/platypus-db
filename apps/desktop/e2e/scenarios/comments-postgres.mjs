// Editing the comments of a table and of a column from the object tab, against the dev
// PostgreSQL from docker/compose.yml. Skipped when it is not running.
import { ALT, CTRL } from "../cdp.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const tableRow = (first) => `[...${objectTab}.querySelectorAll('.meta-table tbody tr')].find(r => r.children[0].textContent === ${JSON.stringify(first)})`;
const columnRow = (name) => `[...${objectTab}.querySelectorAll('.meta-table tbody tr')].find(r => r.children[1].textContent === ${JSON.stringify(name)})`;
const commentCell = `${columnRow("note")}.children[6]`;
const tableComment = `${tableRow("Comment")}.children[1]`;
/** `expression`, or false while what it reads is not there yet. */
const present = (expression) => `(() => { try { return ${expression}; } catch { return false; } })()`;
const field = `document.querySelector('.sql-action textarea')`;
const ready = (text) =>
  `document.querySelector('.sql-preview').textContent.includes(${JSON.stringify(`IS '${text}'`)}) && !document.querySelector('.sql-action button[type=submit]').disabled`;

async function rightClick(page, label) {
  await page.eval(`${row(label)}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

async function choose(page, item) {
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === ${JSON.stringify(item)}).click()`,
  );
}

async function section(page, name) {
  await page.eval(`[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith(${JSON.stringify(name)})).click()`);
}

/** Types `text` in the open comment dialog, waits for its statement, saves it with `submit`. */
async function comment(page, text, submit) {
  await page.waitFor(`${field} !== null`);
  await page.eval(`${field}.focus()`);
  await page.type(text);
  await page.waitFor(`${field}.value === ${JSON.stringify(text)}`);
  // The statement of the whole text: the one of an earlier keystroke is not run.
  await page.waitFor(ready(text), 15_000);
  await submit();
  await page.waitFor(`document.querySelector('.sql-action') === null`, 15_000);
}

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

  // The new table appears once the schema is read: opened now (it was not), or reloaded.
  if (await page.eval(`${row("demo")}.getAttribute('aria-expanded') === 'true'`)) {
    await rightClick(page, "demo");
    await choose(page, "Refresh");
  } else {
    await page.eval(`${row("demo")}.querySelector(".twisty").click()`);
  }
  await page.waitFor(`${row("e2e_comments")} !== undefined`, 15_000);
  await rightClick(page, "e2e_comments");
  await choose(page, "Open details");
  await page.waitFor(`${objectTab} != null`, 15_000);

  // The table's comment, on two lines; Ctrl+Enter saves it, then it shows.
  await page.waitFor(present(`${tableComment}.querySelector('button') !== null`), 15_000);
  await page.eval(`${tableComment}.querySelector('button').click()`);
  await comment(page, "Owned by sales\nand finance", () => page.key("Enter", "Enter", 13, CTRL));
  await page.waitFor(present(`${tableComment}.textContent.includes('Owned by sales\\nand finance')`), 15_000);

  // A column's comment, saved with the button.
  await section(page, "Columns");
  await page.waitFor(present(`${commentCell}.textContent === 'Edit'`), 15_000);
  await page.eval(`${commentCell}.querySelector('button').click()`);
  await comment(page, "free text", () =>
    page.eval(`[...document.querySelectorAll('.sql-action button')].find(b => b.textContent === 'Save comment').click()`),
  );
  await page.waitFor(present(`${commentCell}.textContent.includes('free text')`), 15_000);

  // A new query tab for the clean-up.
  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await run(page, "DROP TABLE demo.e2e_comments;");
}
