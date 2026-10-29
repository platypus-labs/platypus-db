// What the object creation scenarios (scenarios/schema-objects-*.mjs) share: opening the
// Create submenu of a schema, filling the fields of the dialog and reading its preview.
import { CTRL } from "./cdp.mjs";

export const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
export const form = `document.querySelector('.modal.object-form')`;
const field = (name) => `${form}.querySelector('[name=${JSON.stringify(name)}]')`;
export const notice = `(${form}?.querySelector('.notice.error')?.textContent ?? '')`;
export const preview = `(${form}?.querySelector('.sql-preview')?.textContent ?? '')`;
export const createEnabled = `(${form}?.querySelector('button[name=create]')?.disabled === false)`;
export const editorText = `[...document.querySelectorAll('.cm-content')].map(e => e.textContent).join('\\n')`;

/** Right-clicks the navigator row `label` and waits for its menu. */
export async function rightClick(page, label) {
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id'); ${row(label)}.scrollIntoView({ block: 'center' }); ${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  const r = await page.rect("#e2e-row");
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.click(r.x + 5, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

/** Opens the Create submenu of the context menu and returns its items. */
export async function createMenu(page, schema) {
  await rightClick(page, schema);
  await page.eval(`[...document.querySelectorAll('.context-menu [aria-haspopup]')].find(b => b.querySelector('span').textContent === 'Create').click()`);
  await page.waitFor(`document.querySelector('.context-menu.submenu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu.submenu button span:first-child')].map(s => s.textContent)`);
}

/** Opens the dialog of the item `item` of the schema's Create submenu. */
export async function openCreate(page, schema, item) {
  await createMenu(page, schema);
  await page.eval(
    `[...document.querySelectorAll('.context-menu.submenu button')].find(b => b.querySelector('span').textContent === ${JSON.stringify(item)}).click()`,
  );
  await page.waitFor(`${form} !== null`);
}

/** Opens Create trigger… of the table `table`. */
export async function openTableTrigger(page, table) {
  await rightClick(page, table);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Create trigger…').click()`);
  await page.waitFor(`${form} !== null`);
}

/** Replaces the text of the field `name` with `text` and waits until the field holds it. */
export async function fill(page, name, text) {
  await page.eval(`${field(name)}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.waitFor(`${field(name)}.value === ${JSON.stringify(text)}`);
}

/** Sets a select of the dialog; React hears it through the `change` event. */
export async function choose(page, name, value) {
  await page.eval(`(() => {
    const s = ${field(name)};
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(value)});
    s.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.waitFor(`${field(name)}.value === ${JSON.stringify(value)}`);
}

/** Ticks or unticks a checkbox of the dialog. */
export async function tick(page, name, on) {
  await page.eval(`(() => { const c = ${field(name)}; if (c.checked !== ${on}) c.click(); })()`);
  await page.waitFor(`${field(name)}.checked === ${on}`);
}

/** The options of a select of the dialog. */
export const options = (page, name) => page.eval(`[...${field(name)}.options].map(o => o.textContent)`);

/** Waits until the preview shows `text` and Create is allowed (the statements are for the
 *  fields as they are now, not for an earlier keystroke). */
export async function previewHas(page, text) {
  await page.waitFor(`${preview}.includes(${JSON.stringify(text)}) && ${createEnabled}`, 15_000);
}

/** Waits until the dialog says `message` (the preview is regenerated after a pause). */
export async function errorIs(page, message) {
  await page.waitFor(`${notice}.includes(${JSON.stringify(message)})`, 15_000);
}

/** Clicks Create and waits for the dialog to go. */
export async function create(page) {
  await page.eval(`${field("create")}.click()`);
  await page.waitFor(`${form} === null`, 20_000);
}

/** Shows the folder `folder` of the schema (opening it when closed) and waits for `name` in it. */
export async function reveal(page, folder, name) {
  await page.waitFor(`${row(folder)} !== undefined`, 20_000);
  if (await page.eval(`${row(folder)}.getAttribute('aria-expanded') !== 'true'`)) {
    await page.eval(`${row(folder)}.querySelector('.twisty').click()`);
  }
  await page.waitFor(`${row(name)} !== undefined`, 20_000);
}
