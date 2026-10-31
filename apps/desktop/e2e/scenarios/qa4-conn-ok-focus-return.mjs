// Regression guard (keyboard use): the "+" of the navigator opened with Enter and the picker
// closed with Esc give the focus back to the "+", so the next Tab goes on from there.
import assert from "node:assert/strict";
import { openPicker, picker, seedSqlite } from "../conn-helpers.mjs";
import { press } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  const plus = `document.querySelector('button[title="New connection"]')`;
  await page.eval(`${plus}.focus()`);
  await press(page, "Enter", "Enter", 13);
  await page.waitFor(`${picker} !== null`);
  await page.sleep(500);
  await press(page, "Escape", "Escape", 27);
  await page.waitFor(`${picker} === null`);
  const focused = await page.eval(`document.activeElement === ${plus} ? 'plus' : document.activeElement.tagName + ':' + (document.activeElement.title || document.activeElement.textContent.slice(0, 20))`);
  assert.equal(focused, "plus", `after Esc the focus is on ${focused}`);
}
