// What the qa4-ana2 scenarios share: the notebook's gestures (cells, running, typing), on a
// SQLite connection made by the scenario.
import { existsSync, readFileSync } from "node:fs";
import { CTRL } from "./cdp.mjs";
import { treeMenu } from "./qa-grid.mjs";

export const tab = `[...document.querySelectorAll('.notebook-tab')].find(t => t.style.display !== 'none')`;
export const cell = (name) => `${tab}?.querySelector('[data-name=${JSON.stringify(name)}]')`;
export const cells = `[...${tab}.querySelectorAll('.notebook-cell')].map(c => c.dataset.name ?? 'text')`;
export const statusOf = (name) => `(${cell(name)}?.querySelector('[data-field=status]')?.textContent ?? '')`;
export const errorOf = (name) => `(${cell(name)}?.querySelector('[data-field=error]')?.textContent ?? '')`;
export const resultOf = (name) => `(${cell(name)}?.querySelector('[data-field=status]')?.dataset.result ?? '')`;
export const click = (scope, action) => `${scope}.querySelector('[data-action=${JSON.stringify(action)}]').click()`;

/** Types `sql` as the whole text of the cell `name`. */
export async function typeIn(page, name, sql) {
  await page.eval(`${cell(name)}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${cell(name)}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
}

/** Runs `action` and waits until the cell `name` has a new result or another error. */
export async function ran(page, name, action) {
  const before = await page.eval(resultOf(name));
  const error = await page.eval(errorOf(name));
  await action();
  await page.waitFor(
    `(() => { const e = ${errorOf(name)}; return !${cell(name)}.classList.contains('running') && (${resultOf(name)} !== ${JSON.stringify(before)} || (e !== '' && e !== ${JSON.stringify(error)})); })()`,
    30_000,
  );
}

export const run = (page, name) => ran(page, name, () => page.eval(click(cell(name), "run")));

export async function addSql(page, sql) {
  const before = await page.eval(`${tab}.querySelectorAll('.notebook-cell.sql').length`);
  await page.eval(click(tab, "add_sql"));
  const name = `c${before + 1}`;
  await page.waitFor(`${cell(name)}?.querySelector('.cm-content') != null`);
  await typeIn(page, name, sql);
  return name;
}

export async function newNotebook(page, connection) {
  const before = await page.eval(`document.querySelectorAll('.notebook-tab').length`);
  await treeMenu(page, connection, "New notebook");
  await page.waitFor(`document.querySelectorAll('.notebook-tab').length > ${before} && ${cell("c1")} != null`);
}

/** Waits until `file` is written whole, as `done` tells from its text, and returns the text. */
export async function written(page, file, done) {
  let text = "";
  for (let i = 0; i < 100; i++) {
    text = existsSync(file) ? readFileSync(file, "utf8") : "";
    if (done(text)) return text;
    await page.sleep(100);
  }
  throw new Error(`${file} not written: ${text.length} characters`);
}

// ---- Dashboards: scripts are files of the All connections folder, linked to a connection.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setField } from "./plans.mjs";

export const dtab = `[...document.querySelectorAll('.dashboard-tab')].find(t => t.style.display !== 'none')`;
export const widget = (title) => `${dtab}?.querySelector('[data-title=${JSON.stringify(title)}]')`;
export const atOf = (title) => `(${widget(title)}?.querySelector('[data-field=status]')?.dataset.at ?? '')`;
export const dStatusOf = (title) => `(${widget(title)}?.querySelector('[data-field=status]')?.textContent ?? '')`;
export const dErrorOf = (title) => `(${widget(title)}?.querySelector('[data-field=error]')?.textContent ?? '')`;
export const valueOf = (title) => `(${widget(title)}?.querySelector('[data-field=value]')?.textContent ?? '')`;
export const titles = `[...${dtab}.querySelectorAll('.dashboard-widget')].map(w => w.dataset.title)`;

/** Writes `scripts` (file → SQL) in the All connections folder, linked to the connection `id`. */
export function writeScripts(dir, id, scripts) {
  const folder = join(dir, "scripts", "all");
  mkdirSync(folder, { recursive: true });
  for (const [file, sql] of Object.entries(scripts)) writeFileSync(join(folder, file), sql);
  writeFileSync(join(folder, ".links.json"), JSON.stringify(Object.fromEntries(Object.keys(scripts).map((f) => [f, id]))));
}

export async function fillForm(page, w) {
  await page.waitFor(`document.querySelector('.dashboard-form [data-field=script] option[value=${JSON.stringify(w.script)}]') !== null`, 15_000);
  for (const field of ["script", "title", "connection", "view", "rows", "width", "height"]) {
    if (w[field] !== undefined) await setField(page, `.dashboard-form [data-field=${field}]`, String(w[field]));
  }
  await page.eval(`document.querySelector('.dashboard-form [data-action=save]').click()`);
  await page.waitFor(`document.querySelector('.dashboard-form') === null`);
}

export async function addWidget(page, w) {
  const title = w.title;
  const count = await page.eval(`${titles}.length`);
  await page.eval(click(dtab, "add"));
  await page.waitFor(`document.querySelector('.dashboard-form') !== null`);
  await fillForm(page, w);
  await page.waitFor(`${titles}.length === ${count + 1} && ${atOf(title)} !== ''`, 30_000);
}

export async function newDashboard(page, connection) {
  const before = await page.eval(`document.querySelectorAll('.dashboard-tab').length`);
  await treeMenu(page, connection, "New dashboard");
  await page.waitFor(`document.querySelectorAll('.dashboard-tab').length > ${before} && ${dtab} != null`);
}
