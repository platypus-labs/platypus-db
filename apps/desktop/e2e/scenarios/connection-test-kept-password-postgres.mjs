// Test connection on a saved connection whose form was changed. Another database of the same
// server keeps the saved password. Another server does not get it: the form asks for it, says
// why, and the password typed stays in the field for Save. Left empty, the test fails with
// the server's answer; back on the saved server, the saved password works again.
import assert from "node:assert/strict";
import { button, dialog, editConnection, fill, fillForm, formField, menu, menuItem, newConnection, saveForm, seedSqlite, testConnection, treeRow } from "../conn-helpers.mjs";
import { needsPostgres } from "../journey-helpers.mjs";

export const skip = needsPostgres;

export function setup({ dir }) {
  seedSqlite(dir);
}

const NAME = "E2E kept password";
const prompt = `[...document.querySelectorAll('.modal-backdrop form.modal h2')].find(h => h.textContent.startsWith('Password for'))?.closest('form')`;
const notice = `document.querySelector('.connection-dialog .notice.ok, .connection-dialog .notice.error')`;

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await fillForm(page, { Name: NAME, Host: "localhost", Port: 55432, Database: "platypus", User: "platypus", Password: "platypus" });
  assert.match(await testConnection(page), /^Connected/);
  await saveForm(page);
  await page.waitFor(`${treeRow(NAME)} !== undefined`);

  await editConnection(page, NAME);
  await page.waitFor(`${formField("Password")}.placeholder.includes('saved')`);
  assert.match(await testConnection(page), /^Connected/, "the form as saved");

  // Another database of the same server: no question, the helper's empty answer would fail it.
  await fillForm(page, { Database: "postgres" });
  assert.match(await testConnection(page), /^Connected/, "another database");
  await fillForm(page, { Database: "platypus" });

  // Another server: asked, with the reason, and what is typed stays for Save.
  await fillForm(page, { Host: "127.0.0.1" });
  await page.eval(`${button(dialog, "Test connection")}.click()`);
  await page.waitFor(`${prompt} != null`, 30_000);
  assert.match(await page.eval(`${prompt}.innerText`), /only goes to the server it was saved for/);
  assert.doesNotMatch(await page.eval(`${prompt}.innerText`), /No password is saved/);
  await page.eval(fill(`${prompt}.querySelector('input[type=password]')`, "platypus"));
  await page.eval(`${prompt}.requestSubmit()`);
  await page.waitFor(`${notice} !== null && !${button(dialog, "Test connection")}.disabled`, 30_000);
  assert.match(await page.eval(`${notice}.textContent`), /^Connected/);
  assert.equal(await page.eval(`${formField("Password")}.value`), "platypus");

  // Nothing typed: the test fails, with what the server said.
  await fillForm(page, { Password: "" });
  assert.doesNotMatch(await testConnection(page), /^Connected/, "the saved password must stay home");

  // Back on the saved server, nothing to type.
  await fillForm(page, { Host: "localhost" });
  assert.match(await testConnection(page), /^Connected/, "the saved server");

  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-kept'`);
  await menu(page, "#e2e-kept");
  await menuItem(page, "Delete");
  await page.waitFor(`[...document.querySelectorAll('.modal button')].some(b => b.textContent.trim() === 'Delete')`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Delete').click()`);
  await page.waitFor(`${treeRow(NAME)} === undefined`);
}
