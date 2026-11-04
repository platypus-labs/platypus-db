// The driver a new connection needs is offered when the database is chosen in the picker, not
// only at Test connection or the first opening. The profile has no ADBC driver, so choosing
// Trino opens the popup with a Download button (not "Download and connect"); Cancel leaves the
// form open, usable and saveable, and Test connection offers the driver again. PostgreSQL has
// nothing to download: no popup. Nothing is downloaded.
import assert from "node:assert/strict";
import { button, dialog, driverPopup, fill, formField, openPicker, picker, saveForm, seedSqlite, treeRow } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const pick = (page, label) =>
  page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
const popupText = `(${driverPopup}?.textContent ?? '')`;

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined`);

  // Trino, no driver: the popup opens over the form at once.
  await openPicker(page);
  await page.waitFor(`[...document.querySelectorAll('.driver-tile')].some(b => b.textContent === 'Trino')`, 15_000);
  await pick(page, "Trino");
  await page.waitFor(`${dialog} !== null && ${driverPopup} !== null`, 15_000);
  assert.ok((await page.eval(popupText)).includes("Trino needs a driver"));
  // The button says Download (no connection exists yet) and the footer is not a pointer to Settings.
  const buttons = await page.eval(`[...${driverPopup}.querySelectorAll('.actions button')].map(b => b.textContent.trim())`);
  assert.deepEqual(buttons, ["Cancel", "Download"]);
  assert.ok((await page.eval(popupText)).includes("You can also do it later"));
  assert.ok(!(await page.eval(popupText)).includes("Settings"));
  assert.equal(await page.eval(`document.querySelectorAll('.driver-popup').length`), 1);

  // Cancel: the form is there and works.
  await page.eval(`${button(driverPopup, "Cancel")}.click()`);
  await page.waitFor(`${driverPopup} === null`);
  assert.notEqual(await page.eval(dialog), null);
  await page.eval(fill(formField("Name"), "Trino later"));
  await page.eval(fill(formField("Host"), "127.0.0.1"));
  assert.equal(await page.eval(`${formField("Name")}.value`), "Trino later");

  // Test connection finds the driver missing and offers it again, now with "connect".
  await page.eval(`${button(dialog, "Test connection")}.click()`);
  await page.waitFor(`${driverPopup} !== null`, 15_000);
  assert.ok((await page.eval(popupText)).includes("Trino needs a driver"));
  assert.equal(await page.eval(`${button(driverPopup, "Download and connect")} !== undefined`), true);
  await page.eval(`${button(driverPopup, "Cancel")}.click()`);
  await page.waitFor(`${driverPopup} === null`);

  // The connection is saved all the same.
  await saveForm(page);
  await page.waitFor(`${treeRow("Trino later")} !== undefined`);

  // PostgreSQL: nothing to download, no popup (given the lookup time to answer).
  await openPicker(page);
  await page.waitFor(`[...document.querySelectorAll('.driver-tile')].some(b => b.textContent === 'PostgreSQL')`, 15_000);
  await pick(page, "PostgreSQL");
  await page.waitFor(`${dialog} !== null`);
  await page.sleep(1500);
  assert.equal(await page.eval(`${driverPopup} === null && ${picker} === null`), true);
}
