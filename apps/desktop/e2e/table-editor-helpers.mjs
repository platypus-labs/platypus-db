// Driving the table editor tab, for the navigator QA scenarios.
import { CTRL } from "./cdp.mjs";

export const editor = `[...document.querySelectorAll('.table-editor')].find(t => t.style.display !== 'none')`;
export const modal = `document.querySelector('.modal.sql-action')`;
const body = `${editor}.querySelector('.object-body')`;

/** The input of row `r`, column `c` (0-based) of the section's table. */
export const cell = (r, c) => `${body}.querySelectorAll('tbody tr')[${r}].querySelectorAll('td')[${c}].querySelector('input, select')`;

/** Shows the section whose button starts with `label` (Columns, Indexes, Foreign keys, Constraints, SQL). */
export async function section(page, label) {
  await page.eval(`[...${editor}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith(${JSON.stringify(label)})).click()`);
  await page.waitFor(`[...${editor}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith(${JSON.stringify(label)})).classList.contains('active')`);
}

/** Replaces the text of the input `expression` by typing `text` (empty: clears it). */
export async function fill(page, expression, text) {
  await page.eval(`(${expression}).id = 'e2e-input'`);
  await page.eval(`document.getElementById('e2e-input').scrollIntoView({ block: 'center' })`);
  await page.clickSel("#e2e-input");
  await page.key("a", "KeyA", 65, CTRL);
  if (text === "") await page.key("Delete", "Delete", 46);
  else await page.type(text);
  // Leaving the field: the column lists are read on blur.
  await page.eval(`document.getElementById('e2e-input')?.blur(); document.getElementById('e2e-input')?.removeAttribute('id')`);
}

/** Clicks the checkbox `expression`. */
export async function check(page, expression) {
  await page.eval(`(${expression}).click()`);
}

/** Sets the select `expression` to `value`. */
export async function choose(page, expression, value) {
  await page.eval(`(() => { const s = ${expression}; s.value = ${JSON.stringify(value)}; s.dispatchEvent(new Event('change', { bubbles: true })); })()`);
}

/** Adds a row to the section shown. */
export async function addRow(page) {
  const n = await page.eval(`${body}.querySelectorAll('tbody tr').length`);
  await page.eval(`${editor}.querySelector('.add-row').click()`);
  await page.waitFor(`${body}.querySelectorAll('tbody tr').length === ${n + 1}`);
}

/** Opens the Save/Create dialog and returns the script it shows. */
export async function openSave(page) {
  await page.waitFor(`${editor}.querySelector('.object-header button.primary')?.disabled === false`, 10_000);
  await page.eval(`${editor}.querySelector('.object-header button.primary').click()`);
  await page.waitFor(`${modal}?.querySelector('pre') != null`);
  return page.eval(`${modal}.querySelector('pre').textContent`);
}

/** Confirms the dialog; returns the error it shows, or null once it closed. */
export async function confirm(page) {
  await page.eval(`${modal}.querySelector('button[type=submit]').click()`);
  await page.waitFor(`${modal} === null || ${modal}.querySelector('.notice.error') !== null`, 20_000);
  return page.eval(`${modal}?.querySelector('.notice.error')?.textContent ?? null`);
}

/** The warnings the editor's preview shows. */
export const warnings = `[...(${modal} ?? ${editor}).querySelectorAll('.notice.warn')].map(w => w.textContent)`;
/** The error of the editor's preview or load. */
export const editorError = `${editor}?.querySelector('.object-body > .notice.error, .notice.error')?.textContent ?? null`;
