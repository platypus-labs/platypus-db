// Helpers of the Users and roles scenarios (group J): the admin tab's section, the list and
// details of users and roles, and the dialogs that create, change and drop them and grant
// and revoke privileges.
import { spawnSync } from "./proc.mjs";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { assert } from "./grid-helpers.mjs";
import { adminTab, openAdmin, showSection } from "./admin.mjs";

export const usersView = `${adminTab}.querySelector('[data-view="users"]')`;

/** The list row of the user or role `name`. */
export const principalRow = (name) => `${usersView}?.querySelector('tr.admin-principal[data-name="${name}"]')`;

/** The details of the selected user or role. */
export const details = `${adminTab}.querySelector('.admin-details')`;

/** A field of the details (`host`, `limit`, `attributes`, `member-of`, `members`, …). */
export const detailField = (key) => `${details}.querySelector('[data-field="${key}"]')`;

/** A button of the details (`edit`, `privileges`, `roles`, `drop`). */
export const detailButton = (action) => `${details}.querySelector('button[data-action="${action}"]')`;

/** The open dialog of the Users and roles view. */
export const dialog = `document.querySelector('.modal.security-dialog')`;

/** An element of the open dialog. */
export const inDialog = (selector) => `${dialog}.querySelector(${JSON.stringify(selector)})`;

/** Connects `name`, opens Server activity on it and switches to Users and roles. */
export async function openUsers(page, name, db) {
  await openAdmin(page, name, db);
  await showSection(page, "Users and roles");
  await page.waitFor(`${usersView} !== null && ${usersView}.querySelector('tr.admin-principal') !== null`, 20_000);
}

/** Selects the user or role `name` and waits for its details. */
export async function selectPrincipal(page, name) {
  await page.waitFor(`${principalRow(name)} !== null`, 15_000);
  await page.eval(`${principalRow(name)}.click()`);
  await page.waitFor(`${details}?.dataset.principal === ${JSON.stringify(name)}`, 10_000);
}

/** Sets an input or select of the page the way a user does, so the form sees it. */
export function setValue(page, selector, text) {
  return page.eval(`(() => {
    const el = document.querySelector(${JSON.stringify(selector)});
    const select = el instanceof HTMLSelectElement;
    const proto = select ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(text)});
    el.dispatchEvent(new Event(select ? 'change' : 'input', { bubbles: true }));
  })()`);
}

/** Sets a field of the open dialog. */
export const setField = (page, field, text) => setValue(page, `.modal.security-dialog [data-field="${field}"]`, text);

/** Waits for the select `field` of the open dialog to offer `value`: the catalog's schemas and
 *  tables arrive a moment after the dialog opens. */
export const waitOption = (page, field, value) =>
  page.waitFor(`[...(${inDialog(`[data-field="${field}"]`)}?.options ?? [])].some(o => o.value === ${JSON.stringify(value)})`, 15_000);

/** Checks or unchecks a checkbox of the open dialog (`[data-flag=…]`, `[data-privilege=…]`,
 *  `[data-field=…]`) by clicking it only when it is not as asked. */
export async function tick(page, selector, on = true) {
  await page.eval(`(() => {
    const el = ${inDialog(selector)};
    if (el.checked !== ${on}) el.click();
  })()`);
  await page.waitFor(`${inDialog(selector)}.checked === ${on}`);
}

/** The statements the open dialog shows. */
export const previewText = (page) => page.eval(`${inDialog('[data-field="preview"]')}.textContent`);

/** Waits for the preview to hold `text`: it is written a moment after the form changes. */
export async function waitPreview(page, text) {
  await page.waitFor(`${inDialog('[data-field="preview"]')}?.textContent.includes(${JSON.stringify(text)})`, 10_000);
  return previewText(page);
}

/** Whether the dialog's primary button can be pressed. */
export const canRun = (page) => page.eval(`!${inDialog('button[data-action="run"]')}.disabled`);

/** Waits for the dialog's primary button to be enabled, presses it and waits for the dialog
 *  to close. */
export async function runDialog(page) {
  await page.waitFor(`${inDialog('button[data-action="run"]')}?.disabled === false`, 10_000);
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${dialog} === null`, 30_000);
}

/** Closes the open dialog with Cancel. */
export async function cancelDialog(page) {
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await page.waitFor(`${dialog} === null`);
}

/** Opens a dialog from a button of the view and waits for it. */
export async function openDialog(page, buttonExpression) {
  await page.waitFor(`${buttonExpression} !== null && ${buttonExpression}.disabled === false`, 10_000);
  await page.eval(`${buttonExpression}.click()`);
  await page.waitFor(`${dialog} !== null`, 10_000);
}

/** The "New user…" or "New role…" button. */
export const newButton = (of) => `${usersView}.querySelector('button[data-action="new-${of}"]')`;

/** The names of the roles in a details list (`member-of`, `members`). */
export const chips = (page, key) =>
  page.eval(`[...(${detailField(key)}?.querySelectorAll('.users-chip') ?? [])].map(c => c.dataset.name)`);

/** The objects of the privileges in the details, with the privileges of each. */
export const privilegeGroups = (page) =>
  page.eval(`Object.fromEntries([...${details}.querySelectorAll('.users-privileges tr')].map(r => [r.dataset.object, [...r.querySelectorAll('.users-privilege')].map(p => p.textContent.replace(/[ ⚑]+$/, '') + (p.classList.contains('grantable') ? '*' : ''))]))`);

/** The texts of the app's query log. */
export const logTexts = (page) =>
  page.eval(`window.__TAURI_INTERNALS__.invoke('query_log', { since: 0 }).then(p => (p.entries ?? p).map(e => e.text))`);

/** Checks that the log holds `statement` and that no entry holds `secret`. */
export async function checkLogged(page, statement, secret) {
  const texts = await logTexts(page);
  assert.ok(texts.some((t) => t.includes(statement)), `the log holds ${statement}: ${texts.slice(-8).join(" | ")}`);
  assert.ok(texts.every((t) => !t.includes(secret)), "a password reached the query log");
}

/** Writes connections.toml with `list` of `[name, db, extra]` (extra: lines to add, e.g.
 *  `read_only = true`), as `connections` does. */
export function usersConnections(dir, list, DBS) {
  let text = "version = 1\n";
  list.forEach(([name, db, extra = ""], i) => {
    const id = `00000000-0000-4000-8000-0000000019${String(i).padStart(2, "0")}`;
    text += `\n[[connections]]\nid = "${id}"\nname = "${name}"\n${DBS[db].toml}\nsave_password = false\n${extra}`;
  });
  writeFileSync(join(dir, "connections.toml"), text);
}

/** Runs `sql` as `root` in the dev MySQL's container: the app's own user may not create
 *  users, so the scenario gives it the rights for its run and takes them back. */
export function mysqlRoot(sql) {
  const r = spawnSync(
    "docker",
    ["exec", "-i", "platypus-db-mysql-1", "mysql", "-uroot", "-pplatypus", "-N", "-B", "--default-character-set=utf8mb4"],
    { input: sql, encoding: "utf8" },
  );
  if (r.status !== 0) throw new Error(`mysql root: ${r.stderr || r.stdout}`);
  return r.stdout.replace(/\r/g, "").trim();
}
