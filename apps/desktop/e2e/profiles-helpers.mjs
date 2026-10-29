// What the transfer profile scenarios (profiles-*) share: the profiles dialog (from the
// palette or a connection's menu), its rows, the "Save as profile" name dialog, and the file
// the profiles are kept in.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { palette } from "./plans.mjs";
import { button, fill, menu } from "./qa-xfer.mjs";

export const profilesDialog = `[...document.querySelectorAll('.schedules-dialog')].find(d => d.querySelector('h2')?.textContent.startsWith('Transfer profiles'))`;
export const nameDialog = `document.querySelector('.modal.name-dialog')`;
export const confirmDialog = `document.querySelector('.modal.confirm')`;

/** The row of profile `name` in the open profiles dialog. */
export const profileRow = (name) =>
  `[...(${profilesDialog}?.querySelectorAll('.schedule-row') ?? [])].find(r => r.querySelector('strong')?.textContent === ${JSON.stringify(name)})`;

/** The names of the profiles listed, in order. */
export const listed = (page) =>
  page.eval(`[...${profilesDialog}.querySelectorAll('.schedule-row strong')].map(s => s.textContent)`);

/** Waits for the list of the open profiles dialog to be read: rows, or the empty notice. */
async function loaded(page) {
  await page.waitFor(`${profilesDialog} !== undefined && (${profilesDialog}.querySelector('.schedule-row') !== null || ${profilesDialog}.textContent.includes('No profiles saved yet.'))`, 15_000);
}

/** Opens the profiles dialog from the command palette. */
export async function openProfiles(page) {
  await palette(page, "Transfer profiles");
  await loaded(page);
}

/** Opens the profiles dialog from the context menu of connection `name`. */
export async function openProfilesOf(page, name) {
  await menu(page, name, "Transfer profiles");
  await loaded(page);
}

/** Closes the profiles dialog. */
export async function closeProfiles(page) {
  await page.eval(`${button(profilesDialog, "Close")}.click()`);
  await page.waitFor(`${profilesDialog} === undefined`);
}

/** Clicks `action` on the row of profile `name`. */
export async function rowAction(page, name, action) {
  await page.waitFor(`${profileRow(name)} !== undefined`, 10_000);
  await page.eval(`${button(profileRow(name), action)}.click()`);
}

/** Opens the dialog afresh (so no run of an earlier time is shown), runs profile `name` and
 *  waits for its outcome: the row's text once it says "Done:" or shows an error. Closes the
 *  dialog again. */
export async function runProfile(page, name) {
  await openProfiles(page);
  await rowAction(page, name, "Run");
  const ended = `(${profileRow(name)}.textContent.includes('Done:') || ${profileRow(name)}.querySelector('.notice.error') !== null)`;
  await page.waitFor(ended, 60_000);
  const text = await page.eval(`${profileRow(name)}.textContent`);
  const error = await page.eval(`${profileRow(name)}.querySelector('.notice.error')?.textContent ?? null`);
  await closeProfiles(page);
  return { text, error };
}

/** Fills the open name dialog with `name` and saves; returns the error it shows, or null
 *  (the dialog is then closed; on an error it stays open). */
export async function nameIt(page, name) {
  await page.waitFor(`${nameDialog} !== null`);
  await page.eval(fill(`${nameDialog}.querySelector('input')`, name));
  await page.waitFor(`${nameDialog}.querySelector('input').value === ${JSON.stringify(name)} && !${button(nameDialog, "Save")}.disabled`);
  await page.eval(`${button(nameDialog, "Save")}.click()`);
  await page.waitFor(`${nameDialog} === null || ${nameDialog}.querySelector('.notice.error') !== null`, 15_000);
  return page.eval(`${nameDialog}?.querySelector('.notice.error')?.textContent ?? null`);
}

/** Cancels the open name dialog. */
export async function cancelName(page) {
  await page.eval(`${button(nameDialog, "Cancel")}.click()`);
  await page.waitFor(`${nameDialog} === null`);
}

/** In `modal` (an expression), presses `label` (a profile button) and names the profile
 *  `name`; waits for the dialog's "Saved as profile" message. Returns the name dialog's error,
 *  or null. */
export async function saveProfile(page, modal, label, name) {
  await page.waitFor(`${button(modal, label)} && !${button(modal, label)}.disabled`, 20_000);
  await page.eval(`${button(modal, label)}.click()`);
  const error = await nameIt(page, name);
  if (error === null) await page.waitFor(`${modal}.textContent.includes(${JSON.stringify(`Saved as profile ${name}.`)})`, 15_000);
  return error;
}

/** The profiles kept in the data folder `dir`, by name. */
export function savedProfiles(dir) {
  const { profiles } = JSON.parse(readFileSync(join(dir, "transfer-profiles.json"), "utf8"));
  return Object.fromEntries(profiles.map((p) => [p.name, p]));
}

/** Deletes profile `name` from the open profiles dialog, confirming; returns the error shown
 *  when the profile cannot be deleted, or null once it is gone. */
export async function deleteProfile(page, name) {
  await rowAction(page, name, "Delete");
  await page.waitFor(`${confirmDialog} !== null`);
  await page.eval(`${confirmDialog}.querySelector('button.danger').click()`);
  await page.waitFor(`${confirmDialog} === null`);
  await page.waitFor(`${profileRow(name)} === undefined || ${profilesDialog}.querySelector('.notice.error') !== null`, 15_000);
  const error = await page.eval(`${profilesDialog}.querySelector('.notice.error')?.textContent ?? null`);
  if (error === null) assert.equal(await page.eval(`${profileRow(name)} === undefined`), true);
  return error;
}
